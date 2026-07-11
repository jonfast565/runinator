import { defineStore } from "pinia";
import { computed, reactive, watch } from "vue";
import type {
  Connection,
  Edge,
  EdgeChange,
  EdgeMouseEvent,
  EdgeUpdateEvent,
  Node,
  NodeChange,
  NodeDragEvent,
  NodeMouseEvent,
} from "@vue-flow/core";
import type {
  ControlFrame,
  DebugFrame,
  GateRecord,
  JsonRecord,
  JsonValue,
  ProviderMetadata,
  RunSummary,
  RuninatorType,
  WorkflowDefinition,
  WorkflowEdgeEditorDraft,
  WorkflowLayoutDirection,
  WorkflowNodeKind,
  WorkflowNodeRun,
  WorkflowRunDetail,
  WorkflowTrigger,
  WorkflowTriggerKind,
  WorkflowValidationIssue,
} from "../../../../core/domain/models";
import type { WorkflowDebugPatch } from "../../../../core/api/commandCenterApi";
import { workflowInputType } from "../../../../core/domain/models";
import { runWorkflowSnapshot } from "../../../../core/domain/models";
import { formatMaybeDate } from "../../../../core/workflow/editor-defaults";
import { workflowRunSearchText, optionIdForSourceHandle } from "../../../../core/workflow/index";
import {
  buildInputSkeleton,
  newWorkflowDraft,
  newWorkflowTriggerDraft,
} from "../../../../core/workflow/editor-defaults";
import { catalogMetadataService, workflowServices } from "../../../../core/services";
import { workflowNodeKindsList } from "../../../../core/workflow";
import { useAppStore } from "../app";
import { useProvidersStore } from "../providers";
import { buildGraphEdges, buildGraphNodes } from "../../vue-flow/builder";
import { mirrorServiceState } from "../sync";

export { buildInputSkeleton, newWorkflowDraft, newWorkflowTriggerDraft } from "../../../../core/workflow/editor-defaults";

const WORKFLOW_WDL_SYNC_DELAY_MS = 1500;

function providerCatalog(): ProviderMetadata[] {
  return useProvidersStore().providers;
}

export const useWorkflowsStore = defineStore("workflows", () => {
  const svc = workflowServices;
  const state = mirrorServiceState(svc);
  const catalogState = mirrorServiceState(catalogMetadataService);
  const app = useAppStore();

  function mirroredComputed<T>(selector: () => T) {
    return computed(() => {
      void state.value;
      return selector();
    });
  }

  const workflowDraft = reactive(svc.getState().workflowDraft);
  const triggerDraft = reactive(svc.getState().triggerDraft);
  const triggerJson = reactive(svc.getState().triggerJson);
  const stepEditor = reactive(svc.getState().stepEditor);

  svc.subscribe(() => {
    const next = svc.getState();
    Object.assign(workflowDraft, next.workflowDraft);
    Object.assign(triggerDraft, next.triggerDraft);
    Object.assign(triggerJson, next.triggerJson);
    Object.assign(stepEditor, next.stepEditor);
  });

  watch(
    () => state.value.workflowJson,
    () => {
      if (svc.internal.workflowJsonWriteGuard || state.value.workflowEditorMode !== "json") {
        return;
      }

      scheduleWorkflowJsonSync();
    },
  );

  watch(
    () => state.value.workflowWdl,
    () => {
      if (svc.internal.workflowWdlWriteGuard || state.value.workflowWdlError) {
        return;
      }

      svc.setState((current) => ({ ...current, workflowEditorMode: "wdl" }));
      scheduleWorkflowWdlSync();
    },
  );

  watch(
    stepEditor,
    () => {
      if (!state.value.stepEditorOpen) {
        return;
      }

      svc.editor.scheduleStepEditorApply();
    },
    { deep: true },
  );

  let workflowWdlSyncTimer: ReturnType<typeof setTimeout> | null = null;

  function scheduleWorkflowJsonSync() {
    void svc.editor.syncWorkflowJson();
  }

  function scheduleWorkflowWdlSync() {
    if (workflowWdlSyncTimer) {
      clearTimeout(workflowWdlSyncTimer);
    }

    workflowWdlSyncTimer = setTimeout(() => {
      workflowWdlSyncTimer = null;
      void svc.editor.syncWorkflowWdl();
    }, WORKFLOW_WDL_SYNC_DELAY_MS);
  }

  const selectedWorkflow = mirroredComputed((): WorkflowDefinition | null => svc.getSelectedWorkflow());
  const canRunWorkflow = mirroredComputed(() => Boolean(selectedWorkflow.value?.enabled && selectedWorkflow.value.id));
  const selectedWorkflowInputType = mirroredComputed((): RuninatorType | null =>
    selectedWorkflow.value ? workflowInputType(selectedWorkflow.value) : null,
  );
  const selectedWorkflowHasInputs = mirroredComputed(() => svc.selectedWorkflowHasInputs());
  const canManageWorkflowTriggers = mirroredComputed(() => Boolean(workflowDraft.id));
  const canStepWorkflowRun = mirroredComputed(() => svc.canStepWorkflowRun());
  const debugState = mirroredComputed<DebugFrame | null>(() => svc.getDebugState());
  const isDebugRun = mirroredComputed(() => svc.isDebugRun());
  const canContinueWorkflowRun = mirroredComputed(() => svc.canContinueWorkflowRun());
  const controlState = mirroredComputed<ControlFrame | null>(() => svc.getControlState());
  const pauseRequested = mirroredComputed(() => Boolean(controlState.value?.pause_requested));
  const canPauseWorkflowRun = mirroredComputed(() => svc.canPauseWorkflowRun());
  const canResumeWorkflowRun = mirroredComputed(() => svc.canResumeWorkflowRun());
  const canCancelWorkflowRun = mirroredComputed(() => svc.canCancelWorkflowRun());
  const currentBreakpoints = mirroredComputed<string[]>(() => svc.getCurrentBreakpoints());
  const isBreakpointed = (nodeId: string) => svc.runs.isBreakpointed(nodeId);
  const selectedStepKindLocked = mirroredComputed(() => {
    const node = svc.getSelectedNode();
    return node ? !svc.canRemoveSelectedStep() && Boolean(node) : false;
  });
  const canRemoveSelectedStep = mirroredComputed(() => svc.canRemoveSelectedStep());
  const filteredWorkflows = mirroredComputed((): WorkflowDefinition[] => svc.getFilteredWorkflows());
  const recentWorkflowRuns = computed((): RunSummary[] => {
    const query = app.normalizedSearch;
    const runs = state.value.workflowRuns;

    if (!query) {
      return runs.slice(0, 50);
    }

    const matches: RunSummary[] = [];

    for (const run of runs) {
      const workflowName = svc.catalog.workflowNameForRun(run);

      if (workflowRunSearchText(run, workflowName).includes(query)) {
        matches.push(run);
      }
    }

    return matches.slice(0, 50);
  });
  const workflowRunDetailText = computed(() => {
    const detail = state.value.workflowRunDetail;

    if (!detail) {
      return "";
    }

    const lines = [
      `Run ${detail.run.id}: ${detail.run.status}`,
      `Started: ${formatMaybeDate(detail.run.started_at)}`,
      `Finished: ${formatMaybeDate(detail.run.finished_at)}`,
    ];

    if (detail.run.message) {
      lines.push(`Message: ${detail.run.message}`);
    }

    for (const step of detail.nodes) {
      lines.push(
        `${step.node_id}: ${step.status}, attempt ${String(step.attempt)}, node run ${step.id}${step.message ? `, ${step.message}` : ""}`,
      );
    }

    return `${lines.join("\n")}${state.value.workflowNodeDetailExtra}`;
  });
  const stepNeeds = computed(() => {
    const nodeDraft = state.value.stepEditor.nodeDraft;
    const transitions =
      nodeDraft && typeof nodeDraft === "object" && !Array.isArray(nodeDraft)
        ? ((nodeDraft).transitions as JsonRecord | undefined) ?? {}
        : {};
    return ["next", "on_success", "on_failure", "on_timeout", "on_reject"]
      .filter((key) => (transitions)[key])
      .map((key) => `${key}:${String((transitions)[key])}`)
      .join(",");
  });
  const subflowNames = mirroredComputed(() => svc.getSubflowNames());
  // workflowDraft is a persistent object mutated in place (Object.assign) rather than replaced,
  // so plain computed()s over it don't reliably track changes to its nested definition/nodes;
  // mirroredComputed's state.value read forces recomputation on every workflow-store notify().
  const graphNodes = mirroredComputed((): Node[] =>
    buildGraphNodes(workflowDraft, null, subflowNames.value, providerCatalog()),
  );
  const graphEdges = mirroredComputed((): Edge[] => buildGraphEdges(workflowDraft));
  const graphValidationIssues = mirroredComputed((): WorkflowValidationIssue[] => svc.getGraphValidationIssues());
  const workflowRunWorkflow = mirroredComputed((): WorkflowDefinition | null => svc.getWorkflowRunWorkflow());
  const workflowRunGatesByNodeId = computed((): Map<string, GateRecord> => {
    const gates = new Map<string, GateRecord>();

    for (const gate of state.value.workflowRunGates) {
      if (typeof gate.node_id === "string" && gate.node_id.length > 0) {
        gates.set(gate.node_id, gate);
      }
    }

    return gates;
  });
  const runGraphNodes = computed((): Node[] => {
    if (!workflowRunWorkflow.value) {
      return [];
    }

    return buildGraphNodes(
      workflowRunWorkflow.value,
      state.value.workflowRunDetail,
      subflowNames.value,
      providerCatalog(),
    ).map((node) => ({
      ...node,
      data: {
        ...(node.data as JsonRecord),
        readOnly: true,
        allowGateResolution: true,
        gate: workflowRunGatesByNodeId.value.get(node.id) ?? null,
      },
    }));
  });
  const runGraphEdges = computed((): Edge[] =>
    workflowRunWorkflow.value ? buildGraphEdges(workflowRunWorkflow.value) : [],
  );
  const selectedNode = mirroredComputed((): JsonRecord | null => svc.getSelectedNode());
  const selectedGraphEdge = computed(
    () => graphEdges.value.find((edge: Edge) => edge.id === state.value.selectedGraphEdgeId) ?? null,
  );
  const selectedNodeIssues = computed<WorkflowValidationIssue[]>(() =>
    graphValidationIssues.value.filter((issue) => issue.nodeId === state.value.selectedStepId),
  );
  const selectedEdgeIssues = computed<WorkflowValidationIssue[]>(() => {
    const edge = selectedGraphEdge.value;

    if (!edge) {
      return [];
    }

    const data = edge.data as { transitionKey?: string; branchIndex?: number; parameterKey?: string; parameterIndex?: number };
    const semanticKey =
      data?.transitionKey ??
      (typeof data?.branchIndex === "number"
        ? `branches.${String(data.branchIndex)}`
        : `${data?.parameterKey ?? ""}${data?.parameterIndex ?? ""}`);
    return graphValidationIssues.value.filter(
      (issue) => issue.edgeKey === `${edge.source}:${semanticKey}`,
    );
  });
  const selectedNodePendingApproval = computed((): WorkflowNodeRun | null => {
    const detail = state.value.workflowRunDetail;

    if (!detail || !state.value.selectedStepId) {
      return null;
    }

    return (
      detail.nodes
        .filter(
          (node) =>
            node.node_id === state.value.selectedStepId &&
            ["waiting", "approval_required", "pending"].includes(node.status),
        )
        .at(-1) ?? null
    );
  });
  const watchExpressionsForActiveWorkflow = computed<string[]>(() => {
    const workflowId = workflowRunWorkflow.value?.id;

    if (!workflowId) {
      return [];
    }

    return state.value.watchExpressionsByWorkflowId[workflowId] ?? [];
  });

  function onGraphNodeClick(event: NodeMouseEvent) {
    const nodeId = event.node.id;

    if (nodeId) {
      svc.editor.dismissStepEditorForCanvasEdit();
      svc.setState((current) => ({
        ...current,
        selectedGraphEdgeId: "",
        inlineEditNodeId: "",
      }));
      svc.editor.populateStepEditor(nodeId);
    }
  }

  function onGraphNodeDoubleClick(event: NodeMouseEvent) {
    const nodeId = event.node.id;

    if (!nodeId) {
      return;
    }

    svc.setState((current) => ({ ...current, selectedGraphEdgeId: "" }));
    svc.editor.populateStepEditor(nodeId);
    svc.setState((current) => ({ ...current, inlineEditNodeId: nodeId }));
  }

  function onGraphNodeDragStop(event: NodeDragEvent) {
    const node = event.node;

    if (!node.id) {
      return;
    }

    svc.editor.dismissStepEditorForCanvasEdit();
    svc.editor.setGraphNodePosition(node.id, node.position);
    svc.editor.syncWorkflowDraftToJson();
  }

  function onGraphNodesChange(changes: NodeChange[]) {
    let changed = false;

    for (const change of changes) {
      if (change.type !== "position" || !change.id || change.dragging) {
        continue;
      }

      svc.editor.setGraphNodePosition(change.id, change.position);
      changed = true;
    }

    if (changed) {
      svc.editor.syncWorkflowDraftToJson();
    }
  }

  function onGraphConnect(connection: Connection) {
    const source = connection.source;
    const handleOptionId = optionIdForSourceHandle(connection.sourceHandle) ?? undefined;
    const options = svc.editor.workflowEdgeOptions(source);

    if (!source || options.length === 0) {
      return;
    }

    const optionId =
      handleOptionId && options.some((option) => option.id === handleOptionId)
        ? handleOptionId
        : options.length === 1
          ? options[0].id
          : "";

    if (optionId) {
      svc.editor.applyGraphEdgeSemantic(connection, optionId);
    }
  }

  function onGraphEdgeClick(event: EdgeMouseEvent) {
    const edgeId = event.edge.id;

    if (edgeId) {
      svc.editor.selectGraphEdge(edgeId);
    }
  }

  function onGraphEdgeUpdate(event: EdgeUpdateEvent) {
    const edge = event.edge;
    const connection = event.connection;

    if (!connection.source || !connection.target) {
      return;
    }

    if (svc.editor.applyGraphEdgeSemantic(connection, edge.id, edge.id)) {
      if (state.value.selectedStepId === edge.source) {
        svc.editor.populateStepEditor(edge.source);
      }
    }

    svc.setState((current) => ({ ...current, selectedGraphEdgeId: "" }));
  }

  function onGraphEdgesChange(changes: EdgeChange[]) {
    for (const change of changes) {
      if (change.type === "remove") {
        svc.editor.removeWorkflowEdgeById(change.id);
      }
    }
  }

  const runDetailById = computed(() => svc.internal.runDetailById);

  return {
    recentWorkflowRuns,
    getTransition: svc.runs.getTransition,
    setTransition: svc.runs.setTransition,
    workflows: computed({
      get: () => state.value.workflows,
      set: (workflows) => { svc.setState((current) => ({ ...current, workflows })); },
    }),
    selectedWorkflowId: computed({
      get: () => state.value.selectedWorkflowId,
      set: (selectedWorkflowId) => { svc.setState((current) => ({ ...current, selectedWorkflowId })); },
    }),
    workflowDraft,
    workflowJson: computed({
      get: () => state.value.workflowJson,
      set: (workflowJson) => { svc.setState((current) => ({ ...current, workflowJson })); },
    }),
    workflowWdl: computed({
      get: () => state.value.workflowWdl,
      set: (workflowWdl) => { svc.setState((current) => ({ ...current, workflowWdl })); },
    }),
    workflowWdlError: computed(() => state.value.workflowWdlError),
    workflowConcurrency: computed({
      get: () => state.value.workflowConcurrency,
      set: (workflowConcurrency) => { svc.setState((current) => ({ ...current, workflowConcurrency })); },
    }),
    workflowSettingsOpen: computed({
      get: () => state.value.workflowSettingsOpen,
      set: (workflowSettingsOpen) => { svc.setState((current) => ({ ...current, workflowSettingsOpen })); },
    }),
    workflowTriggers: computed({
      get: () => state.value.workflowTriggers,
      set: (workflowTriggers) => { svc.setState((current) => ({ ...current, workflowTriggers })); },
    }),
    triggerEditorOpen: computed(() => state.value.triggerEditorOpen),
    triggerEditorCreating: computed(() => state.value.triggerEditorCreating),
    triggerEditorError: computed(() => state.value.triggerEditorError),
    triggerDraft,
    triggerJson,
    workflowEditorMode: computed({
      get: () => state.value.workflowEditorMode,
      set: (workflowEditorMode) => { svc.setState((current) => ({ ...current, workflowEditorMode })); },
    }),
    workflowLayoutDirection: computed({
      get: () => state.value.workflowLayoutDirection,
      set: (workflowLayoutDirection) =>
        { svc.setState((current) => ({ ...current, workflowLayoutDirection })); },
    }),
    workflowInspectorMode: computed(() => state.value.workflowInspectorMode),
    stepEditorOpen: computed(() => state.value.stepEditorOpen),
    stepEditorCreating: computed(() => state.value.stepEditorCreating),
    stepEditorError: computed(() => state.value.stepEditorError),
    workflowRuns: computed({
      get: () => state.value.workflowRuns,
      set: (workflowRuns) => { svc.setState((current) => ({ ...current, workflowRuns })); },
    }),
    workflowLayoutVersion: computed(() => state.value.workflowLayoutVersion),
    selectedWorkflowRunId: computed({
      get: () => state.value.selectedWorkflowRunId,
      set: (selectedWorkflowRunId) => { svc.setState((current) => ({ ...current, selectedWorkflowRunId })); },
    }),
    workflowRunDetail: computed(() => state.value.workflowRunDetail),
    workflowRunGates: computed(() => state.value.workflowRunGates),
    workflowNodeDetailExtra: computed(() => state.value.workflowNodeDetailExtra),
    selectedStepId: computed({
      get: () => state.value.selectedStepId,
      set: (selectedStepId) => { svc.setState((current) => ({ ...current, selectedStepId })); },
    }),
    inlineEditNodeId: computed({
      get: () => state.value.inlineEditNodeId,
      set: (inlineEditNodeId) => { svc.setState((current) => ({ ...current, inlineEditNodeId })); },
    }),
    selectedWorkflowRunNodeId: computed({
      get: () => state.value.selectedWorkflowRunNodeId,
      set: (selectedWorkflowRunNodeId) =>
        { svc.setState((current) => ({ ...current, selectedWorkflowRunNodeId })); },
    }),
    selectedWorkflowNodeRunId: computed({
      get: () => state.value.selectedWorkflowNodeRunId,
      set: (selectedWorkflowNodeRunId) =>
        { svc.setState((current) => ({ ...current, selectedWorkflowNodeRunId })); },
    }),
    stepEditor,
    selectedWorkflow,
    canRunWorkflow,
    selectedWorkflowInputType,
    selectedWorkflowHasInputs,
    runInputOpen: computed(() => state.value.runInputOpen),
    runInputDraft: computed({
      get: () => state.value.runInputDraft,
      set: (runInputDraft) => { svc.setState((current) => ({ ...current, runInputDraft })); },
    }),
    runInputDebug: computed(() => state.value.runInputDebug),
    closeRunInput: svc.runs.closeRunInput,
    confirmRunInput: svc.runs.confirmRunInput,
    canManageWorkflowTriggers,
    canRemoveSelectedStep,
    filteredWorkflows,
    workflowRunDetailText,
    stepNeeds,
    graphNodes,
    graphEdges,
    graphValidationIssues,
    workflowRunWorkflow,
    runGraphNodes,
    runGraphEdges,
    selectedNode,
    selectedStepKindLocked,
    selectedGraphEdgeId: computed({
      get: () => state.value.selectedGraphEdgeId,
      set: (selectedGraphEdgeId) => { svc.setState((current) => ({ ...current, selectedGraphEdgeId })); },
    }),
    selectedGraphEdge,
    selectedNodeIssues,
    selectedEdgeIssues,
    selectedNodePendingApproval,
    canStepWorkflowRun,
    canContinueWorkflowRun,
    canPauseWorkflowRun,
    canResumeWorkflowRun,
    canCancelWorkflowRun,
    debugState,
    controlState,
    isDebugRun,
    currentBreakpoints,
    isBreakpointed,
    workflowNodeKinds: computed(() => {
      void catalogState.value.nodeKinds;
      return workflowNodeKindsList();
    }),
    directTransitionKeys: svc.directTransitionKeys,
    refreshWorkflows: svc.catalog.refreshWorkflows,
    clearServiceState: svc.catalog.clearServiceState,
    selectWorkflow: svc.catalog.selectWorkflow,
    addWorkflow: svc.catalog.addWorkflow,
    saveSelectedWorkflow: svc.catalog.saveSelectedWorkflowBundle,
    deleteSelectedWorkflow: svc.catalog.deleteSelectedWorkflow,
    duplicateSelectedWorkflow: svc.catalog.duplicateSelectedWorkflow,
    runSelectedWorkflow: svc.runs.runSelectedWorkflow,
    runSelectedWorkflowDebug: svc.runs.runSelectedWorkflowDebug,
    stepSelectedWorkflowRun: svc.runs.stepSelectedWorkflowRun,
    continueSelectedWorkflowRun: svc.runs.continueSelectedWorkflowRun,
    pauseSelectedWorkflowRun: svc.runs.pauseSelectedWorkflowRun,
    resumeSelectedWorkflowRun: svc.runs.resumeSelectedWorkflowRun,
    cancelSelectedWorkflowRun: svc.runs.cancelSelectedWorkflowRun,
    patchSelectedWorkflowRunDebug: svc.runs.patchSelectedWorkflowRunDebug,
    toggleBreakpoint: svc.runs.toggleBreakpoint,
    runToCursor: svc.runs.runToCursor,
    skipCurrentNode: svc.runs.skipCurrentNode,
    rerunCurrentNode: svc.runs.rerunCurrentNode,
    replaySelectedWorkflowRun: svc.runs.replaySelectedWorkflowRun,
    renameSelectedWorkflowRun: svc.runs.renameSelectedWorkflowRun,
    openRunIds: computed(() => state.value.openRunIds),
    openRunInTab: svc.runs.openRunInTab,
    closeRunTab: svc.runs.closeRunTab,
    activateRunTab: svc.runs.activateRunTab,
    runDetailById,
    watchExpressionsForActiveWorkflow,
    addWatchExpression: svc.runs.addWatchExpression,
    removeWatchExpression: svc.runs.removeWatchExpression,
    fetchWorkflowRunsForSelected: svc.runs.fetchWorkflowRunsForSelected,
    fetchRecentWorkflowRuns: svc.runs.fetchRecentWorkflowRuns,
    selectWorkflowRun: svc.runs.selectWorkflowRun,
    fetchWorkflowRunDetail: svc.runs.fetchWorkflowRunDetail,
    refreshWorkflowRunGates: svc.runs.refreshWorkflowRunGates,
    resolveWorkflowRunGate: svc.runs.resolveWorkflowRunGate,
    setWorkflowRunDetail: svc.runs.setWorkflowRunDetail,
    selectWorkflowRunNode: svc.runs.selectWorkflowRunNode,
    addWorkflowStep: svc.editor.addWorkflowStep,
    addWorkflowNode: svc.editor.addWorkflowNode,
    addConnectedWorkflowNode: svc.editor.addConnectedWorkflowNode,
    applyInlineNodeEdit: svc.editor.applyInlineNodeEdit,
    clearWorkflowGraphSelection: svc.editor.clearWorkflowGraphSelection,
    submitInlineNodeEdit: svc.editor.submitInlineNodeEdit,
    removeWorkflowStep: svc.editor.removeWorkflowStep,
    removeWorkflowNode: svc.editor.removeWorkflowNode,
    removeWorkflowEdgeById: svc.editor.removeWorkflowEdgeById,
    openEdgeEditorDraft: svc.editor.openEdgeEditorDraft,
    selectGraphEdge: svc.editor.selectGraphEdge,
    applyEdgeEditorDraft: svc.editor.applyEdgeEditorDraft,
    moveEdgeEditorItem: svc.editor.moveEdgeEditorItem,
    moveSelectedEdge: svc.editor.moveSelectedEdge,
    reverseSelectedEdgeHandles: svc.editor.reverseSelectedEdgeHandles,
    setEdgeLabelOffset: svc.editor.setEdgeLabelOffset,
    setEdgeLabelAnchor: svc.editor.setEdgeLabelAnchor,
    workflowEdgeOptions: svc.editor.workflowEdgeOptions,
    applyGraphEdgeSemantic: svc.editor.applyGraphEdgeSemantic,
    applyStepEditor: svc.editor.applyStepEditor,
    populateStepEditor: svc.editor.populateStepEditor,
    updateSelectedWorkflowNodeDetail: svc.runs.updateSelectedWorkflowNodeDetail,
    onGraphNodeClick,
    onGraphNodeDoubleClick,
    onGraphNodeDragStop,
    onGraphNodesChange,
    onGraphConnect,
    onGraphEdgeClick,
    onGraphEdgeUpdate,
    onGraphEdgesChange,
    autoArrangeWorkflowNodes: svc.editor.autoArrangeWorkflowNodes,
    isDirty: computed(() => state.value.isDirty),
    syncWorkflowJson: svc.editor.syncWorkflowJson,
    syncWorkflowDraftToJson: svc.editor.syncWorkflowDraftToJson,
    syncWorkflowWdl: svc.editor.syncWorkflowWdl,
    exportWorkflowWdl: svc.catalog.exportWorkflowWdl,
    exportWorkflowPack: svc.catalog.exportWorkflowPack,
    ensureWorkflowNodes: svc.editor.ensureWorkflowNodes,
    addNodeRefEditor: svc.editor.addNodeRefEditor,
    removeNodeRefEditor: svc.editor.removeNodeRefEditor,
    openStepEditor: svc.editor.openStepEditor,
    closeStepEditor: svc.editor.closeStepEditor,
    submitStepEditor: svc.editor.submitStepEditor,
    duplicateSelectedStep: svc.editor.duplicateSelectedStep,
    moveWorkflowSelection: svc.catalog.moveWorkflowSelection,
    openWorkflowSettings: svc.catalog.openWorkflowSettings,
    closeWorkflowSettings: svc.catalog.closeWorkflowSettings,
    refreshWorkflowTriggers: svc.catalog.refreshWorkflowTriggers,
    addWorkflowTrigger: svc.catalog.addWorkflowTrigger,
    editWorkflowTrigger: svc.catalog.editWorkflowTrigger,
    closeTriggerEditor: svc.catalog.closeTriggerEditor,
    setTriggerKind: svc.catalog.setTriggerKind,
    submitWorkflowTrigger: svc.catalog.submitWorkflowTrigger,
    deleteSelectedWorkflowTrigger: svc.catalog.deleteSelectedWorkflowTrigger,
    triggerCronSummary: svc.catalog.triggerCronSummary,
    triggerDateForInput: svc.catalog.triggerDateForInput,
    markWorkflowDirty: svc.editor.markWorkflowDirty,
  };
});
