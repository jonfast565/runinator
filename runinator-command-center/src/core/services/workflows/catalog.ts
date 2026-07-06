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
  GateRecord, JsonRecord, JsonValue, ProviderMetadata, RunArtifact, RunChunk, RunSummary,
  RuninatorType, WorkflowDefinition, WorkflowEdgeEditorDraft, WorkflowEditorEdgeData,
  WorkflowLayoutDirection, WorkflowNodeKind, WorkflowNodeRun, WorkflowRunDetail, WorkflowTrigger,
  WorkflowTriggerKind, WorkflowValidationIssue,
} from "../../domain/models";
import { asJsonValue, isJsonObject } from "../../domain/json";
import { pretty } from "../../utils/format";
import { cloneJson, parseObject, parseRequiredJson, parseRequiredObject } from "../../utils/json";
import { displayValue, isBlankValue } from "../../utils/values";
import { createZip, type ZipEntry } from "../../utils/zip";
import {
  applyWorkflowEdgeEditorDraft, applyWorkflowInlineNodeEdit, asArray, asRecord, isRecord,
  autoArrangeWorkflowEdgeHandles, autoArrangeWorkflowLayout, createWorkflowNode, directTransitionKeys,
  isSameConnectionPointLoop, nodeRef, nodeRefId, normalizeWorkflowDefinition, parameterSemanticKey,
  removeConditionBranch, removeWorkflowEdge, removeWorkflowEdgeHandles, removeWorkflowNodeReferences,
  setConditionBranch, setWorkflowEdgeHandles, setWorkflowEdgeLabelAnchor, setWorkflowEdgeLabelOffset,
  moveWorkflowEdgeEditorDraft, optionIdForSourceHandle, workflowEdgeOptionId, workflowEdgeEditorDraft,
  workflowEdgeSemanticOptions, uniqueWorkflowNodeId, validateWorkflowReferenceSyntax, valueRef,
  workflowNodeActionConfig, workflowNodeActionInputs,
} from "../../workflow/index";
import type { GraphEdgeLike, GraphEdgeModel } from "../../workflow/graph-model";
import {
  branchPolicyName, boundedIndex, defaultEdgeEditorDraft, defaultTriggerConfiguration, errorMessage,
  formatMaybeDate, dateTimeLocalToIso, isLockedWorkflowNode, isProtectedWorkflowNode, newWorkflowDraft,
  newWorkflowTriggerDraft, nextNodePosition, nodeRefArray, switchCaseEditor, validateJsonValueType,
} from "../../workflow/editor-defaults";
import type { WorkflowServiceHost } from "./host";

const WORKFLOW_WDL_SYNC_DELAY_MS = 1500;
const MAX_OPEN_RUN_TABS = 8;
const WATCH_STORAGE_PREFIX = "runinator.watch.";

export type WorkflowEditorPeer = {
  setWorkflowJsonSilently: (next: string) => void;
  setWorkflowWdlSilently: (next: string) => void;
  refreshWorkflowWdl: () => Promise<void>;
  syncWorkflowJson: () => boolean;
  syncWorkflowWdl: () => Promise<boolean>;
  scheduleWorkflowWdlRefresh: () => void;
};

export type WorkflowRunsPeer = {
  clearWorkflowRunGates: () => void;
};

export function createWorkflowCatalogService(
  host: WorkflowServiceHost,
  editor: WorkflowEditorPeer,
  runs: WorkflowRunsPeer,
) {
  const { deps, internal } = host;

  async function refreshWorkflows() {
    console.info("[command-center] refreshing workflows");
    // resolve before touching host.state: notify() elsewhere (e.g. a concurrent
    // fetchRecentWorkflowRuns() in the same Promise.all) can swap the state object out from
    // under a `host.state.x = await ...` assignment, since the getter is read before the await
    // resolves; writing into a local first keeps the final assignment on the live object.
    const fetched = (await host.ctx
      .runOperation("Refreshing workflows", () => fetchWorkflows())
      .catch(() => [])) as WorkflowDefinition[];
    host.state.workflows = fetched;

    if (!host.state.selectedWorkflowId && host.state.workflows.length > 0) {
      host.state.selectedWorkflowId = host.state.workflows[0].id;
    }

    const items = host.state.workflows;
    let workflow: WorkflowDefinition | undefined;

    for (const item of items) {
      if (item.id === host.state.selectedWorkflowId) {
        workflow = item;
        break;
      }
    }

    workflow ??= items[0];

    if (workflow && !host.state.isDirty) {
      await selectWorkflow(workflow);
    }
    host.notify();
  }

  function clearServiceState(options: { discardDraft?: boolean } = {}) {
    host.state.workflows = [];
    host.state.workflowRuns = [];
    host.state.workflowRunDetail = null;
    host.state.openRunIds = [];
    internal.runDetailById.clear();
    internal.pendingBreakpointPatch = null;
    host.state.workflowNodeDetailExtra = "";
    host.state.selectedWorkflowRunId = null;
    host.state.selectedWorkflowRunNodeId = "";
    host.state.selectedWorkflowNodeRunId = null;
    runs.clearWorkflowRunGates();
    clearWorkflowTriggerState();

    if (host.state.isDirty && !options.discardDraft) {
      return;
    }

    host.state.isDirty = false;
    host.state.selectedWorkflowId = null;
    Object.assign(host.state.workflowDraft, newWorkflowDraft());
    editor.setWorkflowJsonSilently(pretty(host.state.workflowDraft.definition));
    editor.setWorkflowWdlSilently("");
    host.state.workflowWdlError = "";
    host.state.selectedStepId = "";
    host.state.stepEditorOpen = false;
    host.notify();
  }

  function selectWorkflow(workflow: WorkflowDefinition) {
    const isSwitch = host.state.selectedWorkflowId !== workflow.id;
    host.state.selectedWorkflowId = workflow.id;
    Object.assign(host.state.workflowDraft, normalizeWorkflowDefinition(cloneJson(workflow)));
    host.state.workflowConcurrency = Number(host.state.workflowDraft.definition.concurrency ?? 1);
    editor.setWorkflowJsonSilently(pretty(host.state.workflowDraft.definition));

    if (isSwitch) {
      host.state.selectedStepId = "";
      clearWorkflowTriggerState();
      host.state.stepEditorOpen = false;
    }

    host.state.workflowEditorMode = "graph";
    host.state.isDirty = false;
    host.notify();
    // the graph derives from the draft; the wdl pane is decompiled, so refresh it for the newly
    // selected workflow since both panes are visible at once.
    return editor.refreshWorkflowWdl();
  }

  function addWorkflow() {
    const workflow = newWorkflowDraft();
    host.state.workflows.push(workflow);
    void selectWorkflow(workflow);
    host.notify();
  }

  function workflowNameForRun(run: RunSummary): string {
    return host.state.workflows.find((workflow) => workflow.id === run.workflow_id)?.name ?? "";
  }

  async function exportWorkflowWdl(): Promise<void> {
    try {
      const source = await decompileToWdl(cloneJson(host.state.workflowDraft));
      const name = (host.state.workflowDraft.name.trim() || "workflow");
      const fileName = `${name.replace(/[^a-z0-9._-]+/gi, "_")}.wdl`;
      host.deps.downloadTextFile(fileName, source, "text/plain");
      host.ctx.setStatus(`Exported ${fileName}`);
    } catch (err) {
      host.ctx.setError(`Could not export this workflow as WDL (${errorMessage(err)}).`);
    }
    host.notify();
  }

  async function exportWorkflowPack(): Promise<void> {
    const allWorkflows = host.state.workflows.filter(
      (workflow): workflow is WorkflowDefinition & { id: string } => workflow.id != null,
    );

    if (allWorkflows.length === 0) {
      host.ctx.setError("No workflows to export.");
      return;
    }

    await host.ctx.runOperation("Exporting workflow pack", async () => {
      const entries: ZipEntry[] = [];
      const manifestWorkflows: string[] = [];
      const triggers: WorkflowTrigger[] = [];
      const usedNames = new Set<string>();
      const skipped: string[] = [];

      for (const workflow of allWorkflows) {
        let source: string;

        try {
          source = await decompileToWdl(cloneJson(workflow));
        } catch {
          skipped.push(workflow.name || `workflow ${workflow.id}`);
          continue;
        }

        let slug = (workflow.name.trim() || `workflow-${workflow.id}`).replace(
          /[^a-z0-9._-]+/gi,
          "_",
        );

        while (usedNames.has(slug)) {
          slug = `${slug}_${workflow.id}`;
        }

        usedNames.add(slug);
        const fileName = `${slug}.wdl`;
        entries.push({ name: fileName, content: source });
        manifestWorkflows.push(fileName);
        triggers.push(...(await fetchWorkflowTriggers(workflow.id).catch(() => [])));
      }

      if (entries.length === 0) {
        throw new Error("no workflows could be decompiled to WDL");
      }

      const manifest = { version: 1, workflows: manifestWorkflows, triggers };
      entries.unshift({ name: "pack.wdlp", content: pretty(manifest) });
      host.deps.downloadBlob("runinator-pack.zip", createZip(entries));
      const note = skipped.length
        ? ` (skipped ${String(skipped.length)} non-WDL: ${skipped.join(", ")})`
        : "";
      host.ctx.setStatus(`Exported ${String(entries.length - 1)} workflow(s) to runinator-pack.zip${note}`);
    });
    host.notify();
  }

  function moveWorkflowSelection(delta: number) {
    const list = host.getFilteredWorkflows();

    if (list.length === 0) {
      return;
    }

    const current = list.findIndex((workflow) => workflow.id === host.state.selectedWorkflowId);
    void selectWorkflow(list[boundedIndex(current, delta, list.length)]);
    host.notify();
  }

  function openWorkflowSettings() {
    host.state.workflowSettingsOpen = true;
    void refreshWorkflowTriggers();
    host.notify();
  }

  function closeWorkflowSettings() {
    host.state.workflowSettingsOpen = false;
    closeTriggerEditor();
    host.notify();
  }

  async function refreshWorkflowTriggers() {
    const workflowId = host.state.workflowDraft.id;

    if (!workflowId) {
      host.state.workflowTriggers = [];
      closeTriggerEditor();
      return;
    }

    const triggers = (await host.ctx
      .runOperation("Loading workflow triggers", () => fetchWorkflowTriggers(workflowId))
      .catch(() => [])) as WorkflowTrigger[];
    host.state.workflowTriggers = triggers;
    host.notify();
  }

  function clearWorkflowTriggerState() {
    host.state.workflowTriggers = [];
    closeTriggerEditor();
    host.notify();
  }

  function addWorkflowTrigger(kind: WorkflowTriggerKind = "cron") {
    if (!host.state.workflowDraft.id) {
      return;
    }

    Object.assign(host.state.triggerDraft, newWorkflowTriggerDraft(host.state.workflowDraft.id, kind));
    host.state.triggerJson.configuration = pretty(host.state.triggerDraft.configuration);
    host.state.triggerJson.metadata = pretty(host.state.triggerDraft.metadata);
    host.state.triggerEditorCreating = true;
    host.state.triggerEditorError = "";
    host.state.triggerEditorOpen = true;
    host.notify();
  }

  function editWorkflowTrigger(trigger: WorkflowTrigger) {
    Object.assign(host.state.triggerDraft, cloneJson(trigger));
    host.state.triggerDraft.next_execution = triggerDateForInput(trigger.next_execution);
    host.state.triggerDraft.blackout_start = triggerDateForInput(trigger.blackout_start);
    host.state.triggerDraft.blackout_end = triggerDateForInput(trigger.blackout_end);
    host.state.triggerJson.configuration = pretty(trigger.configuration);
    host.state.triggerJson.metadata = pretty(trigger.metadata);
    host.state.triggerEditorCreating = false;
    host.state.triggerEditorError = "";
    host.state.triggerEditorOpen = true;
    host.notify();
  }

  function closeTriggerEditor() {
    host.state.triggerEditorOpen = false;
    host.state.triggerEditorCreating = false;
    host.state.triggerEditorError = "";
    host.notify();
  }

  function setTriggerKind(kind: WorkflowTriggerKind) {
    host.state.triggerDraft.kind = kind;

    if (host.state.triggerEditorCreating) {
      host.state.triggerDraft.configuration = defaultTriggerConfiguration(kind);
      host.state.triggerJson.configuration = pretty(host.state.triggerDraft.configuration);
    }
    host.notify();
  }

  async function submitWorkflowTrigger() {
    host.state.triggerEditorError = "";

    if (!host.state.workflowDraft.id) {
      return;
    }

    const configuration = parseRequiredObject(host.state.triggerJson.configuration);
    const metadata = parseRequiredObject(host.state.triggerJson.metadata);

    if (!configuration || !metadata) {
      host.state.triggerEditorError = configuration
        ? "Trigger metadata must be a JSON object"
        : "Trigger configuration must be a JSON object";
      host.ctx.setError(host.state.triggerEditorError);
      return;
    }

    const trigger: WorkflowTrigger = {
      ...cloneJson(host.state.triggerDraft),
      workflow_id: host.state.workflowDraft.id,
      configuration,
      metadata,
      next_execution: dateTimeLocalToIso(host.state.triggerDraft.next_execution),
      blackout_start: dateTimeLocalToIso(host.state.triggerDraft.blackout_start),
      blackout_end: dateTimeLocalToIso(host.state.triggerDraft.blackout_end),
    };
    const saved = await host.ctx.runOperation("Saving workflow trigger", () =>
      saveWorkflowTrigger(trigger, host.state.triggerEditorCreating),
    );
    host.ctx.setStatus(`Workflow trigger saved: ${saved.kind}`);
    closeTriggerEditor();
    await refreshWorkflowTriggers();
    host.notify();
  }

  async function deleteSelectedWorkflowTrigger(trigger: WorkflowTrigger) {
    const triggerId = trigger.id;

    if (!triggerId) {
      return;
    }

    if (!host.deps.confirm(`Delete ${trigger.kind} trigger ${triggerId}?`)) {
      return;
    }

    const response = await host.ctx.runOperation("Deleting workflow trigger", () =>
      deleteWorkflowTrigger(triggerId),
    );

    if (!response.success) {
      host.ctx.setError(response.message || "Failed to delete workflow trigger");
      return;
    }

    host.ctx.setStatus(response.message || "Workflow trigger deleted");

    if (host.state.triggerDraft.id === trigger.id) {
      closeTriggerEditor();
    }

    await refreshWorkflowTriggers();
  }

  function triggerCronSummary(trigger: WorkflowTrigger): string {
    const cron = trigger.configuration.cron;
    return typeof cron === "string" && cron.trim() ? cron : "";
  }

  function triggerDateForInput(value: string | null | undefined): string {
    if (!value) {
      return "";
    }

    const date = new Date(value);

    if (Number.isNaN(date.getTime())) {
      return "";
    }

    const offset = date.getTimezoneOffset() * 60000;
    return new Date(date.getTime() - offset).toISOString().slice(0, 16);
  }

  function workflowSaveTriggers(workflowId: string | null | undefined): WorkflowTrigger[] {
    if (workflowId == null) {
      return [];
    }

    return host.state.workflowTriggers
      .filter((trigger) => trigger.workflow_id === workflowId)
      .map((trigger) => cloneJson(trigger));
  }

  async function workflowWdlSaveRequest(): Promise<WorkflowWdlSaveRequest> {
    const workflow = cloneJson(host.state.workflowDraft);
    const workflowId = workflow.id ?? null;
    const source = await decompileToWdl(workflow);
    const triggers = workflowId === null ? [] : workflowSaveTriggers(workflowId);
    const request: WorkflowWdlSaveRequest = {
      source,
      enabled: workflow.enabled,
      workflow_id: workflowId,
      triggers,
    };

    if (isJsonObject(workflow.definition.ui as JsonValue)) {
      request.ui = cloneJson(workflow.definition.ui) as JsonRecord;
    }

    return request;
  }

  async function saveSelectedWorkflowBundle() {
    const synced =
      host.state.workflowEditorMode === "wdl" ? await editor.syncWorkflowWdl() : editor.syncWorkflowJson();

    if (!synced) {
      return;
    }

    host.state.workflowDraft.definition.concurrency = host.state.workflowConcurrency;
    Object.assign(host.state.workflowDraft, normalizeWorkflowDefinition(cloneJson(host.state.workflowDraft)));
    const saved = await host.ctx.runOperation("Saving workflow", async () =>
      saveWorkflowWdl(await workflowWdlSaveRequest()),
    );
    const savedWorkflow = saved.workflows.at(0);

    if (savedWorkflow === undefined) {
      host.ctx.setError("Workflow bundle save returned no workflow");
      return;
    }

    Object.assign(host.state.workflowDraft, normalizeWorkflowDefinition(cloneJson(savedWorkflow)));
    host.store.setState((current) => ({
      ...current,
      workflowTriggers: saved.triggers.filter(
        (trigger) => trigger.workflow_id === host.state.workflowDraft.id,
      ),
    }));
    editor.setWorkflowJsonSilently(pretty(host.state.workflowDraft.definition));
    editor.scheduleWorkflowWdlRefresh();
    host.ctx.setStatus(`Workflow saved: ${savedWorkflow.name}`);
    host.state.isDirty = false;
    host.state.selectedWorkflowId = savedWorkflow.id;
    await refreshWorkflows();
    host.notify();
  }

  async function deleteSelectedWorkflow() {
    const workflow = host.getSelectedWorkflow();

    if (!workflow?.id) {
      return;
    }

    if (
      !host.deps.confirm(
        `Delete workflow "${workflow.name}"?\n\nThis permanently deletes the workflow along with ALL of its runs and their execution history. This cannot be undone.`,
      )
    ) {
      return;
    }

    const workflowId = workflow.id;
    const response = await host.ctx.runOperation(`Deleting workflow ${workflow.name}`, () =>
      deleteWorkflow(workflowId),
    );

    if (!response.success) {
      host.ctx.setError(response.message || "Failed to delete workflow");
      return;
    }

    host.ctx.setStatus(response.message || `Workflow deleted: ${workflow.name}`);
    closeWorkflowSettings();
    const deletedId = workflow.id;
    host.state.workflows = host.state.workflows.filter((item) => item.id !== deletedId);
    host.state.selectedWorkflowId = host.state.workflows[0]?.id ?? null;

    if (host.state.workflows[0]) {
      await selectWorkflow(host.state.workflows[0]);
    } else {
      Object.assign(host.state.workflowDraft, newWorkflowDraft());
      editor.setWorkflowJsonSilently(pretty(host.state.workflowDraft.definition));
      editor.setWorkflowWdlSilently("");
      host.state.workflowWdlError = "";
      host.state.workflowRuns = [];
      host.state.workflowRunDetail = null;
      host.state.selectedWorkflowRunId = null;
      host.state.isDirty = false;
    }
    host.notify();
  }

  async function duplicateSelectedWorkflow(bump: "major" | "minor" | "patch" = "minor") {
    const workflow = host.getSelectedWorkflow();

    if (!workflow?.id) {
      return;
    }

    if (host.state.isDirty) {
      host.ctx.setError("Save or discard the current changes before duplicating this workflow.");
      return;
    }

    const workflowId = workflow.id;
    const copy = await host.ctx
      .runOperation(`Duplicating workflow ${workflow.name}`, () =>
        duplicateWorkflow(workflowId, bump),
      )
      .catch((error: unknown) => {
        host.ctx.setError(error instanceof Error ? error.message : "Failed to duplicate workflow");
        return null;
      });

    if (!copy) {
      return;
    }

    await refreshWorkflows();
    host.state.selectedWorkflowId = copy.id;
    await selectWorkflow(copy);
    host.ctx.setStatus(`Duplicated ${workflow.name} as v${copy.version}`);
    host.notify();
  }

  return { refreshWorkflows, clearServiceState, selectWorkflow, addWorkflow, workflowNameForRun, exportWorkflowWdl, exportWorkflowPack, moveWorkflowSelection, openWorkflowSettings, closeWorkflowSettings, refreshWorkflowTriggers, clearWorkflowTriggerState, addWorkflowTrigger, editWorkflowTrigger, closeTriggerEditor, setTriggerKind, submitWorkflowTrigger, deleteSelectedWorkflowTrigger, triggerCronSummary, triggerDateForInput, workflowSaveTriggers, workflowWdlSaveRequest, saveSelectedWorkflowBundle, deleteSelectedWorkflow, duplicateSelectedWorkflow };
}
