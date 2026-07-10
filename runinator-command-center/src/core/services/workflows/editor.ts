import {
  cancelWorkflowRun, closeGate, compileWdl, continueWorkflowRun, createWorkflowRun, decompileToWdl,
  deleteWorkflow, deleteWorkflowTrigger, duplicateWorkflow, fetchGates, fetchWorkflowNodeRunArtifacts,
  fetchWorkflowNodeRunChunks, fetchWorkflowRun, fetchWorkflowRuns, fetchWorkflowTriggers, fetchWorkflows,
  openGate, patchWorkflowRunDebug, pauseWorkflowRun, renameWorkflowRun as renameWorkflowRunApi,
  replayWorkflowRun as replayWorkflowRunApi, resumeWorkflowRun, rerunWorkflowNode, runToCursorWorkflowRun,
  saveWorkflowWdl, saveWorkflowTrigger, skipWorkflowNode, stepWorkflowRun,
  type WorkflowDebugPatch, type WorkflowWdlSaveRequest,
} from "../../api/commandCenterApi";
import type {
  GateRecord, JsonRecord, RunArtifact, RunChunk, RunSummary,
  RuninatorType, WorkflowDefinition, WorkflowEdgeEditorDraft, WorkflowEditorEdgeData,
  WorkflowLayoutDirection, WorkflowNodeKind, WorkflowRunDetail, WorkflowTrigger,
  WorkflowTriggerKind,
} from "../../domain/models";
import { pretty } from "../../utils/format";
import { cloneJson, parseObject, parseRequiredObject } from "../../utils/json";
import { displayValue, isBlankValue } from "../../utils/values";
import { createZip, type ZipEntry } from "../../utils/zip";
import {
  applyWorkflowEdgeEditorDraft, applyWorkflowInlineNodeEdit, asArray, asRecord, isRecord,
  autoArrangeWorkflowEdgeHandles, autoArrangeWorkflowLayout, createWorkflowNode, directTransitionKeys,
  isSameConnectionPointLoop, nodeRef, nodeRefId, normalizeWorkflowDefinition, parameterSemanticKey,
  removeWorkflowEdge, removeWorkflowEdgeHandles, removeWorkflowNodeReferences,
  setWorkflowEdgeHandles, setWorkflowEdgeLabelAnchor, setWorkflowEdgeLabelOffset,
  moveWorkflowEdgeEditorDraft, optionIdForSourceHandle, workflowEdgeOptionId, workflowEdgeEditorDraft,
  workflowEdgeSemanticOptions, uniqueWorkflowNodeId, validateWorkflowReferenceSyntax,
} from "../../workflow/index";
import { findNodeKindMetadata } from "../../workflow/catalog-registry";
import { getAtLocation } from "../../workflow/field-location";
import type { GraphEdgeLike, GraphEdgeModel } from "../../workflow/graph-model";
import {
  defaultEdgeEditorDraft, defaultTriggerConfiguration, errorMessage,
  formatMaybeDate, dateTimeLocalToIso, isLockedWorkflowNode, isProtectedWorkflowNode, newWorkflowDraft,
  newWorkflowTriggerDraft, nextNodePosition, validateJsonValueType,
} from "../../workflow/editor-defaults";
import type { WorkflowServiceHost } from "./host";

const WORKFLOW_WDL_SYNC_DELAY_MS = 1500;
const MAX_OPEN_RUN_TABS = 8;
const WATCH_STORAGE_PREFIX = "runinator.watch.";

export interface WorkflowCatalogPeer {
  saveSelectedWorkflowBundle: () => Promise<void>;
}

export interface WorkflowRunsPeer {
  clearWorkflowRunGates: () => void;
  updateSelectedWorkflowNodeDetail: () => Promise<void>;
}

export function createWorkflowEditorService(
  host: WorkflowServiceHost,
  runs: WorkflowRunsPeer,
  catalog: WorkflowCatalogPeer,
) {
  const { deps, internal } = host;

  function addWorkflowStep() {
    addWorkflowNode("action");
  }

  function addWorkflowNode(kind: WorkflowNodeKind) {
    const nodes = ensureWorkflowNodes();
    const newNode = createWorkflowNode(kind, nodes);
    stripNewNodeConnections(newNode);
    const position = graphCentroidPosition();
    const endIndex = nodes.findIndex((node: JsonRecord) => node.kind === "end");

    if (endIndex >= 0) {
      nodes.splice(endIndex, 0, newNode);
    } else {
      nodes.push(newNode);
    }

    setGraphNodePosition(displayValue(newNode.id), position);
    syncWorkflowDraftToJson();
    populateStepEditor(displayValue(newNode.id));
    openStepEditor(displayValue(newNode.id), true);
  }

  function addConnectedWorkflowNode(kind: WorkflowNodeKind = "action") {
    addWorkflowNode(kind);
  }

  function removeWorkflowStep() {
    if (!host.state.selectedStepId || !host.canRemoveSelectedStep()) {
      return;
    }

    removeWorkflowNode(host.state.selectedStepId);
  }

  function removeWorkflowNode(nodeId: string) {
    const node = ensureWorkflowNodes().find((item: JsonRecord) => item.id === nodeId);

    if (!node || isLockedWorkflowNode(node)) {
      return;
    }

    host.state.workflowDraft.definition.nodes = ensureWorkflowNodes().filter(
      (item: JsonRecord) => item.id !== nodeId,
    );
    removeWorkflowNodeReferences(host.state.workflowDraft.definition, nodeId);
    const layout = asRecord(asRecord(host.state.workflowDraft.definition.ui).layout);
    const layoutNodes = asRecord(layout.nodes);
    layout.nodes = Object.fromEntries(
      Object.entries(layoutNodes).filter(([entryId]) => entryId !== nodeId),
    );

    if (host.state.selectedStepId === nodeId) {
      host.state.selectedStepId = "";
    }

    syncWorkflowDraftToJson();
  }

  function applyInlineNodeEdit(nodeId: string, nextId: string, inlineValue: string): boolean {
    const previousId = nodeId;
    const result = applyWorkflowInlineNodeEdit(
      host.state.workflowDraft.definition,
      nodeId,
      nextId,
      inlineValue,
    );

    if (!result.ok) {
      host.ctx.setError(result.message);
      return false;
    }

    if (previousId !== result.nodeId) {
      renameLayoutNode(previousId, result.nodeId);
    }

    host.state.selectedStepId = result.nodeId;
    syncWorkflowDraftToJson();
    populateStepEditor(result.nodeId);
    return true;
  }

  function clearWorkflowGraphSelection() {
    host.state.selectedStepId = "";
    host.state.inlineEditNodeId = "";
    host.state.selectedGraphEdgeId = "";
  }

  function submitInlineNodeEdit(nodeId: string, nextId: string, inlineValue: string): boolean {
    if (!applyInlineNodeEdit(nodeId, nextId, inlineValue)) {
      return false;
    }

    clearWorkflowGraphSelection();
    return true;
  }

  function applyStepEditor(): boolean {
    if (internal.stepEditorApplyTimer) {
      clearTimeout(internal.stepEditorApplyTimer);
      internal.stepEditorApplyTimer = null;
    }

    host.state.stepEditorError = "";

    if (!host.state.selectedStepId) {
      return false;
    }

    const nodes = ensureWorkflowNodes();
    const index = nodes.findIndex((node: JsonRecord) => node.id === host.state.selectedStepId);

    if (index < 0) {
      return false;
    }

    if (isLockedWorkflowNode(nodes[index]) && host.state.stepEditor.kind !== nodes[index].kind) {
      const message = `${String(nodes[index].kind)} node kind cannot be changed`;
      host.state.stepEditorError = message;
      host.ctx.setError(message);
      return false;
    }

    // start from the working copy of the full node json.
    type EditableNode = JsonRecord & {
      id?: string;
      action?: JsonRecord;
      parameters?: JsonRecord;
      transitions?: JsonRecord;
      wait?: JsonRecord;
      retry?: JsonRecord;
    };
    const next = cloneJson(host.state.stepEditor.nodeDraft) as EditableNode;

    next.id = host.state.stepEditor.id.trim();

    if (!next.id) {
      host.state.stepEditorError = "Step ID is required";
      return false;
    }

    const trimmedName = host.state.stepEditor.name.trim();

    if (trimmedName) {
      next.name = trimmedName;
    } else {
      delete next.name;
    }

    next.kind = host.state.stepEditor.kind;
    next.retry = { max_attempts: host.state.stepEditor.max_attempts };

    if (host.state.stepEditor.timeout_seconds > 0) {
      next.timeout_seconds = host.state.stepEditor.timeout_seconds;
    } else {
      delete next.timeout_seconds;
    }

    if (isProtectedWorkflowNode(next)) {
      delete next.locked;
    } else if (host.state.stepEditor.locked) {
      next.locked = true;
    } else {
      delete next.locked;
    }

    if (host.state.stepEditor.skipped) {
      next.skipped = true;
    } else {
      delete next.skipped;
    }

    // validate action provider parameters when a typed action is selected.
    if (next.kind === "action") {
      const actionDraft = asRecord(next.action);
      const configuration = asRecord(actionDraft.configuration);
      const parameterError = validateStepParameters(
        displayValue(actionDraft.provider),
        displayValue(actionDraft.function),
        configuration,
      );

      if (parameterError) {
        host.state.stepEditorError = parameterError;
        host.ctx.setError(parameterError);
        return false;
      }
    }

    // validate required catalog fields via metadata.
    const kindMeta = findNodeKindMetadata(displayValue(next.kind));

    if (kindMeta) {
      for (const field of kindMeta.fields) {
        if (!field.required) {
          continue;
        }

        const value = getAtLocation(next, field.location);

        if (isBlankValue(value)) {
          const message = `${field.label ?? field.name} is required`;
          host.state.stepEditorError = message;
          host.ctx.setError(message);
          return false;
        }
      }
    }

    nodes[index] = next;

    if (host.state.selectedStepId !== next.id) {
      renameLayoutNode(host.state.selectedStepId, next.id);
    }

    host.state.selectedStepId = next.id;
    syncWorkflowDraftToJson();
    return true;
  }

  function populateStepEditor(nodeId: string) {
    const node = ensureWorkflowNodes().find((item: JsonRecord) => item.id === nodeId);

    if (!node) {
      return;
    }

    const retry = asRecord(node.retry);
    internal.stepEditorHydrating = true;

    if (internal.stepEditorApplyTimer) {
      clearTimeout(internal.stepEditorApplyTimer);
      internal.stepEditorApplyTimer = null;
    }

    host.state.selectedStepId = nodeId;
    host.state.stepEditor.id = nodeId;
    host.state.stepEditor.name = displayValue(node.name);
    host.state.stepEditor.kind = displayValue(node.kind) || "action";
    host.state.stepEditor.locked = isLockedWorkflowNode(node);
    host.state.stepEditor.skipped = node.skipped === true;
    host.state.stepEditor.max_attempts = Number(retry.max_attempts ?? 1);
    host.state.stepEditor.timeout_seconds = Number(
      node.timeout_seconds ?? asRecord(node.action).timeout_seconds ?? 0,
    );
    // nodeDraft is the canonical working copy; the modal reads/writes here.
    host.state.stepEditor.nodeDraft = cloneJson(node);
    host.state.workflowInspectorMode = "step";
    void runs.updateSelectedWorkflowNodeDetail();
    setTimeout(() => {
      internal.stepEditorHydrating = false;
    }, 0);
  }

  function workflowEdgeOptions(sourceId: string) {
    const sourceNode = ensureWorkflowNodes().find((node: JsonRecord) => node.id === sourceId);
    return sourceNode ? workflowEdgeSemanticOptions(sourceNode) : [];
  }

  function openEdgeEditorDraft(edgeId: string): WorkflowEdgeEditorDraft | null {
    const edge = host.buildDraftGraphEdges().find((item: GraphEdgeModel) => item.id === edgeId);
    return edge ? workflowEdgeEditorDraft(host.state.workflowDraft, edge) : null;
  }

  function selectGraphEdge(edgeId: string) {
    host.state.selectedStepId = "";
    host.state.selectedGraphEdgeId = edgeId;
  }

  function applyEdgeEditorDraft(draft: WorkflowEdgeEditorDraft): boolean {
    const previousEdge = draft.edgeId
      ? (host.buildDraftGraphEdges().find((edge: GraphEdgeModel) => edge.id === draft.edgeId) ?? null)
      : null;
    const result = applyWorkflowEdgeEditorDraft(host.state.workflowDraft.definition, previousEdge, draft);

    if (!result.ok) {
      host.ctx.setError(result.message);
      return false;
    }

    syncWorkflowDraftToJson();
    populateStepEditor(draft.source);
    return true;
  }

  function moveEdgeEditorItem(
    draft: WorkflowEdgeEditorDraft,
    direction: -1 | 1,
  ): WorkflowEdgeEditorDraft | null {
    const result = moveWorkflowEdgeEditorDraft(host.state.workflowDraft.definition, draft, direction);

    if (!result.ok) {
      host.ctx.setError(result.message);
      return null;
    }

    syncWorkflowDraftToJson();
    populateStepEditor(draft.source);
    const movedEdge = host.buildDraftGraphEdges().find(
      (edge: GraphEdgeModel) =>
        edge.source === result.draft.source &&
        edge.target === result.draft.target &&
        workflowEdgeOptionId(edge) === result.draft.optionId,
    );
    return movedEdge ? { ...result.draft, edgeId: movedEdge.id } : result.draft;
  }

  function moveSelectedEdge(direction: -1 | 1): boolean {
    const draft = host.state.selectedGraphEdgeId ? openEdgeEditorDraft(host.state.selectedGraphEdgeId) : null;

    if (!draft) {
      return false;
    }

    const moved = moveEdgeEditorItem(draft, direction);

    if (!moved) {
      return false;
    }

    host.state.selectedGraphEdgeId = moved.edgeId;
    return true;
  }

  function reverseSelectedEdgeHandles(): boolean {
    const edge = host.getSelectedGraphEdge();

    if (!edge) {
      return false;
    }

    dismissStepEditorForCanvasEdit();
    const data = edge.data as WorkflowEditorEdgeData | undefined;
    const semanticKey =
      data?.transitionKey ??
      (typeof data?.branchIndex === "number"
        ? `branches.${String(data.branchIndex)}`
        : parameterSemanticKey(data?.parameterKey, data?.parameterIndex));
    setWorkflowEdgeHandles(
      host.state.workflowDraft.definition,
      edge.source,
      semanticKey,
      edge.targetHandle,
      edge.sourceHandle,
      data?.edgeStyle,
    );
    syncWorkflowDraftToJson();
    host.state.selectedGraphEdgeId = "";
    return true;
  }

  function setEdgeLabelOffset(edgeId: string, offset: { x: number; y: number } | null): boolean {
    const edge = host.buildDraftGraphEdges().find((item: GraphEdgeModel) => item.id === edgeId);

    if (!edge) {
      return false;
    }

    dismissStepEditorForCanvasEdit();
    setWorkflowEdgeLabelOffset(host.state.workflowDraft.definition, edge, offset);
    syncWorkflowDraftToJson();
    return true;
  }

  function setEdgeLabelAnchor(edgeId: string, position: number | null): boolean {
    const edge = host.buildDraftGraphEdges().find((item: GraphEdgeModel) => item.id === edgeId);

    if (!edge) {
      return false;
    }

    dismissStepEditorForCanvasEdit();
    setWorkflowEdgeLabelAnchor(
      host.state.workflowDraft.definition,
      edge,
      position === null ? null : { position },
    );
    syncWorkflowDraftToJson();
    return true;
  }

  function scheduleStepEditorApply() {
    void applyStepEditor();
  }

  function applyGraphEdgeSemantic(
    connection: GraphEdgeLike,
    optionId: string,
    previousEdgeId = "",
  ): boolean {
    const { source, target, sourceHandle } = connection;

    if (!source || !target) {
      return false;
    }

    dismissStepEditorForCanvasEdit();

    if (isSameConnectionPointLoop(connection)) {
      host.ctx.setError("Cannot connect a node handle back to itself");
      return false;
    }

    const previousEdge = previousEdgeId
      ? (host.buildDraftGraphEdges().find((edge: GraphEdgeModel) => edge.id === previousEdgeId) ?? null)
      : null;
    const previousDraft = previousEdge
      ? workflowEdgeEditorDraft(host.state.workflowDraft, previousEdge)
      : null;
    const draft: WorkflowEdgeEditorDraft = {
      ...(previousDraft ?? defaultEdgeEditorDraft()),
      edgeId: previousEdgeId,
      source,
      target,
      optionId,
      sourceHandle,
      targetHandle: connection.targetHandle,
    };
    return applyEdgeEditorDraft(draft);
  }

  function removeWorkflowEdgeById(edgeId: string) {
    const edge = host.buildDraftGraphEdges().find((item: GraphEdgeModel) => item.id === edgeId);

    if (!edge) {
      return;
    }

    const sourceNode = ensureWorkflowNodes().find((node: JsonRecord) => node.id === edge.source);

    if (!sourceNode || !removeWorkflowEdge(sourceNode, edge)) {
      return;
    }

    const data = edge.data as WorkflowEditorEdgeData | undefined;

    if (data?.transitionKey) {
      removeWorkflowEdgeHandles(host.state.workflowDraft.definition, edge.source, data.transitionKey);
    }

    if (typeof data?.branchIndex === "number") {
      removeWorkflowEdgeHandles(
        host.state.workflowDraft.definition,
        edge.source,
        `branches.${String(data.branchIndex)}`,
      );
    }

    if (data?.parameterKey) {
      removeWorkflowEdgeHandles(
        host.state.workflowDraft.definition,
        edge.source,
        parameterSemanticKey(data.parameterKey, data.parameterIndex),
      );
    }

    syncWorkflowDraftToJson();

    if (host.state.selectedStepId) {
      populateStepEditor(host.state.selectedStepId);
    }
  }

  function autoArrangeWorkflowNodes(
    direction: WorkflowLayoutDirection = host.state.workflowLayoutDirection,
  ) {
    if (!syncWorkflowJson()) {
      return;
    }

    host.state.workflowLayoutDirection = direction;
    const positions = autoArrangeWorkflowLayout(host.state.workflowDraft.definition, direction);

    for (const [nodeId, position] of Object.entries(positions)) {
      setGraphNodePosition(nodeId, position);
    }

    autoArrangeWorkflowEdgeHandles(host.state.workflowDraft.definition, positions);
    host.state.workflowLayoutVersion += 1;
    syncWorkflowDraftToJson();
  }

  function scheduleWorkflowJsonSync() {
    void syncWorkflowJson();
  }

  function scheduleWorkflowWdlSync() {
    if (internal.workflowWdlSyncTimer) {
      clearTimeout(internal.workflowWdlSyncTimer);
    }

    internal.workflowWdlSyncTimer = setTimeout(() => {
      internal.workflowWdlSyncTimer = null;
      void syncWorkflowWdl();
    }, WORKFLOW_WDL_SYNC_DELAY_MS);
  }

  function scheduleWorkflowWdlRefresh() {
    void refreshWorkflowWdl();
  }

  function setWorkflowJsonSilently(next: string) {
    if (internal.workflowJsonWriteReleaseTimer) {
      clearTimeout(internal.workflowJsonWriteReleaseTimer);
    }

    internal.workflowJsonWriteGuard = true;
    host.state.workflowJson = next;
    host.notify();
    internal.workflowJsonWriteReleaseTimer = setTimeout(() => {
      internal.workflowJsonWriteGuard = false;
      internal.workflowJsonWriteReleaseTimer = null;
    }, 0);
  }

  function setWorkflowWdlSilently(next: string) {
    if (internal.workflowWdlWriteReleaseTimer) {
      clearTimeout(internal.workflowWdlWriteReleaseTimer);
    }

    internal.workflowWdlWriteGuard = true;
    host.state.workflowWdl = next;
    host.notify();
    internal.workflowWdlWriteReleaseTimer = setTimeout(() => {
      internal.workflowWdlWriteGuard = false;
      internal.workflowWdlWriteReleaseTimer = null;
    }, 0);
  }

  function syncWorkflowJson(): boolean {
    const parsed = parseRequiredObject(host.state.workflowJson);

    if (!parsed) {
      host.ctx.setError("Workflow definition must be a JSON object");
      return false;
    }

    const errors = validateWorkflowReferenceSyntax(parsed);

    if (errors.length > 0) {
      host.ctx.setError(errors[0]);
      return false;
    }

    host.state.workflowDraft.definition = parsed;
    host.state.workflowDraft.definition.concurrency = host.state.workflowConcurrency;
    Object.assign(host.state.workflowDraft, normalizeWorkflowDefinition(cloneJson(host.state.workflowDraft)));
    setWorkflowJsonSilently(pretty(host.state.workflowDraft.definition));
    host.state.isDirty = true;
    scheduleWorkflowWdlRefresh();
    return true;
  }

  function syncWorkflowDraftToJson() {
    // a graph edit is now the source of truth, so save should serialize the draft, not recompile wdl.
    host.state.workflowEditorMode = "graph";
    host.state.workflowDraft.definition.concurrency = host.state.workflowConcurrency;
    Object.assign(host.state.workflowDraft, normalizeWorkflowDefinition(cloneJson(host.state.workflowDraft)));
    setWorkflowJsonSilently(pretty(host.state.workflowDraft.definition));
    host.state.isDirty = true;
    scheduleWorkflowWdlRefresh();
  }

  async function syncWorkflowWdl(): Promise<boolean> {
    if (internal.workflowWdlSyncTimer) {
      clearTimeout(internal.workflowWdlSyncTimer);
      internal.workflowWdlSyncTimer = null;
    }

    let compiled: WorkflowDefinition;
    const previousUi = isJsonObject(host.state.workflowDraft.definition.ui)
      ? cloneJson(host.state.workflowDraft.definition.ui)
      : null;

    try {
      compiled = await compileWdl(host.state.workflowWdl, host.state.workflowDraft.enabled);
    } catch (err) {
      host.ctx.setError(`WDL compile error: ${errorMessage(err)}`);
      return false;
    }

    host.state.workflowDraft.name = compiled.name;
    host.state.workflowDraft.version = compiled.version;
    host.state.workflowDraft.input_type = compiled.input_type;
    host.state.workflowDraft.definition = compiled.definition;

    if (previousUi) {
      host.state.workflowDraft.definition.ui = previousUi;
    }

    host.state.workflowDraft.definition.concurrency = host.state.workflowConcurrency;
    Object.assign(host.state.workflowDraft, normalizeWorkflowDefinition(cloneJson(host.state.workflowDraft)));
    setWorkflowJsonSilently(pretty(host.state.workflowDraft.definition));
    host.state.isDirty = true;
    return true;
  }

  async function refreshWorkflowWdl(): Promise<void> {
    try {
      setWorkflowWdlSilently(await decompileToWdl(cloneJson(host.state.workflowDraft)));
      host.state.workflowWdlError = "";
    } catch (err) {
      setWorkflowWdlSilently("");
      host.state.workflowWdlError = errorMessage(err);
    }

    host.notify();
  }

  function ensureWorkflowNodes(): JsonRecord[] {
    if (!Array.isArray(host.state.workflowDraft.definition.nodes)) {
      host.state.workflowDraft.definition.nodes = [];
    }

    return host.state.workflowDraft.definition.nodes as JsonRecord[];
  }

  function stripNewNodeConnections(node: JsonRecord) {
    const transitions = asRecord(node.transitions);
    const omitTransitionKeys = new Set<string>([...directTransitionKeys, "branches"]);
    node.transitions = Object.fromEntries(
      Object.entries(transitions).filter(([entryKey]) => !omitTransitionKeys.has(entryKey)),
    );

    const parameters = asRecord(node.parameters);
    const omitParameterKeys = new Set(["target", "default", "body", "catch", "finally"]);
    const cleanedParameters = Object.fromEntries(
      Object.entries(parameters).filter(([entryKey]) => !omitParameterKeys.has(entryKey)),
    );

    if (Array.isArray(parameters.cases)) {
      cleanedParameters.cases = [];
    }

    if (Array.isArray(parameters.branches)) {
      cleanedParameters.branches = [];
    }

    if (Array.isArray(parameters.wait_for)) {
      cleanedParameters.wait_for = [];
    }

    node.parameters = cleanedParameters;
  }

  function graphCentroidPosition(): { x: number; y: number } {
    const positioned = host
      .buildDraftGraphNodes()
      .map((node) => ({
        x: node.position.x,
        y: node.position.y,
      }))
      .filter((position) => Number.isFinite(position.x) && Number.isFinite(position.y));

    if (positioned.length === 0) {
      return nextNodePosition(1);
    }

    const totals = positioned.reduce(
      (sum, position) => ({ x: sum.x + position.x, y: sum.y + position.y }),
      { x: 0, y: 0 },
    );
    return {
      x: Math.round(totals.x / positioned.length),
      y: Math.round(totals.y / positioned.length),
    };
  }

  function setGraphNodePosition(nodeId: string, position: { x: number; y: number }) {
    const definition = host.state.workflowDraft.definition;
    const ui = asRecord(definition.ui);
    definition.ui = ui;
    const layout = asRecord(ui.layout);
    ui.layout = layout;
    const layoutNodes = asRecord(layout.nodes);
    layout.nodes = layoutNodes;
    layoutNodes[nodeId] = { x: position.x, y: position.y };
  }

  function renameLayoutNode(previousId: string, nextId: string) {
    if (!previousId || previousId === nextId) {
      return;
    }

    const layout = asRecord(asRecord(host.state.workflowDraft.definition.ui).layout);
    const layoutNodes = asRecord(layout.nodes);

    if (!layoutNodes[previousId]) {
      return;
    }

    const { [previousId]: movedNode, ...remainingNodes } = layoutNodes;
    layout.nodes = { ...remainingNodes, [nextId]: movedNode };
  }

  function addNodeRefEditor(list: string[]) {
    list.push("");
    markWorkflowDirty();
  }

  function removeNodeRefEditor(list: string[], index: number) {
    list.splice(index, 1);
    markWorkflowDirty();
  }

  function markWorkflowDirty() {
    host.state.isDirty = true;
  }

  function openStepEditor(nodeId: string, creating = false) {
    internal.stepEditorBaselineDefinition = creating ? null : cloneJson(host.state.workflowDraft.definition);
    populateStepEditor(nodeId);
    host.state.stepEditorCreating = creating;
    host.state.stepEditorCreatedNodeId = creating ? nodeId : "";
    host.state.stepEditorError = "";
    host.state.workflowInspectorMode = "step";
    // the full modal supersedes the inline mini-editor.
    host.state.inlineEditNodeId = "";
    host.state.stepEditorOpen = true;
  }

  async function submitStepEditor() {
    if (!applyStepEditor()) {
      return;
    }

    host.state.stepEditorOpen = false;
    host.state.stepEditorCreating = false;
    host.state.stepEditorCreatedNodeId = "";
    host.state.selectedStepId = "";
    host.state.inlineEditNodeId = "";
    // applying a step persists the workflow so canvas edits do not need a manual save.
    await catalog.saveSelectedWorkflowBundle();
  }

  function dismissStepEditorForCanvasEdit() {
    if (!host.state.stepEditorOpen || host.state.stepEditorCreating) {
      return;
    }

    host.state.stepEditorOpen = false;
    host.state.stepEditorError = "";
  }

  function closeStepEditor() {
    if (internal.stepEditorApplyTimer) {
      clearTimeout(internal.stepEditorApplyTimer);
      internal.stepEditorApplyTimer = null;
    }

    if (host.state.stepEditorCreating && host.state.stepEditorCreatedNodeId) {
      const nodeId = host.state.stepEditorCreatedNodeId;
      host.state.workflowDraft.definition.nodes = ensureWorkflowNodes().filter(
        (node: JsonRecord) => node.id !== nodeId,
      );
      syncWorkflowDraftToJson();
    } else if (internal.stepEditorBaselineDefinition) {
      host.state.workflowDraft.definition = cloneJson(internal.stepEditorBaselineDefinition);
      syncWorkflowDraftToJson();
    }

    host.state.selectedStepId = "";
    host.state.inlineEditNodeId = "";
    host.state.stepEditorOpen = false;
    host.state.stepEditorCreating = false;
    host.state.stepEditorCreatedNodeId = "";
    host.state.stepEditorError = "";
    internal.stepEditorBaselineDefinition = null;
    internal.stepEditorHydrating = false;
  }

  function duplicateSelectedStep() {
    if (!host.state.selectedStepId || !host.canRemoveSelectedStep()) {
      return;
    }

    const nodes = ensureWorkflowNodes();
    const source = nodes.find((node: JsonRecord) => node.id === host.state.selectedStepId);

    if (!source) {
      return;
    }

    const copy = cloneJson(source);
    const copyId = uniqueWorkflowNodeId(nodes, `${String(source.id)}_copy`);
    copy.id = copyId;
    stripNewNodeConnections(copy);
    const position = graphCentroidPosition();
    nodes.push(copy);
    setGraphNodePosition(copyId, position);
    syncWorkflowDraftToJson();
    populateStepEditor(copyId);
    openStepEditor(copyId, true);
  }

  function setStepEditorError(message: string) {
    host.state.stepEditorError = message;
    host.ctx.setError(message);
    host.notify();
  }

  function isJsonObject(value: unknown): value is JsonRecord {
    return typeof value === "object" && value !== null && !Array.isArray(value);
  }

  function validateStepParameters(
    providerName: string,
    actionFunction: string,
    configuration: JsonRecord,
  ): string {
    const provider = host.getProviders().find((item) => item.name === providerName);
    const action = provider?.actions.find((item) => item.function_name === actionFunction);

    if (!action) {
      return "Select a valid task provider action";
    }

    for (const parameter of action.parameters) {
      if (!parameter.required) {
        continue;
      }

      const value = configuration[parameter.name];

      if (isBlankValue(value)) {
        return `${parameter.label ?? parameter.name} is required`;
      }

      const typeError = validateJsonValueType(
        value,
        parameter.ty,
        parameter.label ?? parameter.name,
      );

      if (typeError) {
        return typeError;
      }
    }

    return "";
  }

  return { addWorkflowStep, addWorkflowNode, addConnectedWorkflowNode, removeWorkflowStep, removeWorkflowNode, applyInlineNodeEdit, clearWorkflowGraphSelection, submitInlineNodeEdit, applyStepEditor, populateStepEditor, workflowEdgeOptions, openEdgeEditorDraft, selectGraphEdge, applyEdgeEditorDraft, moveEdgeEditorItem, moveSelectedEdge, reverseSelectedEdgeHandles, setEdgeLabelOffset, setEdgeLabelAnchor, scheduleStepEditorApply, applyGraphEdgeSemantic, removeWorkflowEdgeById, autoArrangeWorkflowNodes, scheduleWorkflowJsonSync, scheduleWorkflowWdlSync, scheduleWorkflowWdlRefresh, setWorkflowJsonSilently, setWorkflowWdlSilently, syncWorkflowJson, syncWorkflowDraftToJson, syncWorkflowWdl, refreshWorkflowWdl, ensureWorkflowNodes, stripNewNodeConnections, graphCentroidPosition, setGraphNodePosition, renameLayoutNode, addNodeRefEditor, removeNodeRefEditor, markWorkflowDirty, openStepEditor, submitStepEditor, dismissStepEditorForCanvasEdit, closeStepEditor, duplicateSelectedStep, setStepEditorError, isJsonObject, validateStepParameters };
}
