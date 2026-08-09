import type {
  ControlFrame,
  DebugFrame,
  JsonRecord,
  RuninatorType,
  WorkflowDefinition,
  WorkflowValidationIssue,
} from "../../domain/models";
import { runWorkflowSnapshot, workflowInputType } from "../../domain/models";
import {
  buildCursorMarkers,
  coerceControlFrame,
  coerceDebugFrame,
  coerceRunCursors,
  isCursorPaused,
  type CursorMarker,
  type RunCursor,
} from "../../domain/models/workflow-state";
import {
  asArray,
  buildGraphEdgeModels,
  buildGraphNodeModels,
  directTransitionKeys,
  isRecord,
  validateWorkflowIssues,
  workflowNodeKindsList,
} from "../../workflow/index";
import { interruptDeclarations } from "../../workflow/interrupt-regions";
import type { GraphEdgeModel, GraphNodeModel } from "../../workflow/graph-model";
import type { AppTab } from "../../navigation/app";
import type { RunOperationOptions, ToastAction } from "../app";
import { isLockedWorkflowNode } from "../../workflow/editor-defaults";
import { createStore } from "../event-bus";
import { createWorkflowCatalogService } from "./catalog";
import { createWorkflowEditorService } from "./editor";
import { createWorkflowHeaderService } from "./header";
import { createWorkflowRunService } from "./runs";
import { createWorkflowServicesInternal, createWorkflowServicesState } from "./state";
import type { WorkflowServiceDeps, WorkflowServiceHost } from "./host";

export type { WorkflowServiceDeps } from "./host";

function defaultConfirm(message: string): boolean {
  const confirmFn = (globalThis as { confirm?: (message: string) => boolean }).confirm;
  return typeof confirmFn === "function" ? confirmFn(message) : true;
}

function defaultDownloadBlob(fileName: string, blob: Blob) {
  if (typeof document === "undefined") {
    return;
  }

  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = fileName;
  anchor.click();
  URL.revokeObjectURL(url);
}

function defaultDownloadTextFile(fileName: string, contents: string, mimeType = "text/plain") {
  defaultDownloadBlob(fileName, new Blob([contents], { type: mimeType }));
}

export function createWorkflowServices(inputDeps: WorkflowServiceDeps) {
  const deps: Required<WorkflowServiceDeps> = {
    confirm: defaultConfirm,
    downloadBlob: defaultDownloadBlob,
    downloadTextFile: defaultDownloadTextFile,
    refreshResources: () => undefined,
    ...inputDeps,
  };
  const store = createStore(createWorkflowServicesState());
  const internal = createWorkflowServicesInternal();
  let state = store.getState();

  function notify() {
    store.setState((current) => ({ ...current }));
  }

  store.subscribe(() => {
    state = store.getState();
  });

  const ctx = {
    runOperation: <T>(label: string, operation: () => Promise<T>, options?: RunOperationOptions) =>
      deps.app.runOperation(label, operation, options),
    setStatus: (text: string) => {
      deps.app.setStatus(text);
    },
    setError: (text: string, action?: ToastAction) => {
      deps.app.setError(text, action);
    },
    get normalizedSearch() {
      return deps.app.normalizedSearch;
    },
    get activeTab() {
      return deps.app.getState().activeTab;
    },
    set activeTab(tab: AppTab) {
      deps.app.setActiveTab(tab);
    },
  };

  function getSelectedWorkflow(): WorkflowDefinition | null {
    return state.workflows.find((workflow) => workflow.id === state.selectedWorkflowId) ?? null;
  }

  function getSelectedWorkflowInputType(): RuninatorType | null {
    const workflow = getSelectedWorkflow();
    return workflow ? workflowInputType(workflow) : null;
  }

  function selectedWorkflowHasInputs(): boolean {
    const ty = getSelectedWorkflowInputType();
    return ty?.type === "struct" && Object.keys(ty.fields).length > 0;
  }

  function getDebugState(): DebugFrame | null {
    return coerceDebugFrame(state.workflowRunDetail?.run.state?.debug) ?? null;
  }

  function isDebugRun(): boolean {
    return Boolean(getDebugState()?.enabled);
  }

  function getControlState(): ControlFrame | null {
    return coerceControlFrame(state.workflowRunDetail?.run.state?.control) ?? null;
  }

  /** every thread of control the run currently holds, in persisted order. */
  function getCursors(): RunCursor[] {
    return coerceRunCursors(state.workflowRunDetail?.run.state?.cursors);
  }

  /** draw-ready markers for the graph and the cursor rail. */
  function getCursorMarkers(): CursorMarker[] {
    return buildCursorMarkers(getCursors(), getDebugState(), getSelectedCursorId());
  }

  /**
   * the branch the debugger controls act on: the operator's selection while it is still live, else
   * the first parked one, else the primary. mirrors the backend's `resolve_target_cursor`.
   */
  function getSelectedCursorId(): string | null {
    const cursors = getCursors();

    if (cursors.length === 0) {
      return null;
    }

    const chosen = state.selectedCursorId;

    if (chosen && cursors.some((cursor) => cursor.id === chosen)) {
      return chosen;
    }

    const frame = getDebugState();
    const parked = cursors.find((cursor) => isCursorPaused(cursors, cursor.id, frame));
    return (parked ?? cursors[0]).id;
  }

  function getSelectedCursor(): RunCursor | null {
    const id = getSelectedCursorId();
    return getCursors().find((cursor) => cursor.id === id) ?? null;
  }

  /**
   * whether the selected branch is parked.
   *
   * deliberately not `run.status === "debug_paused"`: the run only takes that status once *every*
   * cursor is parked, so gating on it disabled every debug control for the whole of a forked
   * session -- exactly the case this feature exists for.
   */
  function isSelectedCursorPaused(): boolean {
    const cursors = getCursors();
    const id = getSelectedCursorId();

    if (!id) {
      return state.workflowRunDetail?.run.status === "debug_paused";
    }

    return isCursorPaused(cursors, id, getDebugState());
  }

  function canStepWorkflowRun(): boolean {
    return isSelectedCursorPaused();
  }

  function canContinueWorkflowRun(): boolean {
    return isSelectedCursorPaused();
  }

  function canPauseWorkflowRun(): boolean {
    const status = state.workflowRunDetail?.run.status;
    return Boolean(
      status &&
      ["running", "waiting", "approval_required"].includes(status) &&
      !getControlState()?.pause_requested,
    );
  }

  function canResumeWorkflowRun(): boolean {
    const status = state.workflowRunDetail?.run.status;
    return (
      status === "paused" ||
      (status === "debug_paused" && Boolean(getControlState()?.pause_requested))
    );
  }

  function canCancelWorkflowRun(): boolean {
    const status = state.workflowRunDetail?.run.status;

    if (!status) {
      return false;
    }

    return !["succeeded", "failed", "canceled", "timed_out"].includes(status);
  }

  /**
   * may the operator ask this run to raise an interrupt?
   *
   * needs a live run *and* a workflow that declares at least one handler. without a handler the
   * request is recorded and then dropped by the reducer's fail-open path, so offering the button
   * would promise something that silently never happens.
   */
  function canRequestRunInterrupt(): boolean {
    if (!canCancelWorkflowRun()) {
      return false;
    }

    return interruptDeclarations(getWorkflowRunWorkflow()).length > 0;
  }

  /** the sources this run's workflow declares a handler for, plus the always-requestable ones. */
  function getRequestableInterruptSources(): string[] {
    const declared = interruptDeclarations(getWorkflowRunWorkflow()).map((entry) => entry.source);
    return [...new Set([...declared, "external"])];
  }

  function getCurrentBreakpoints(): string[] {
    return getDebugState()?.breakpoints ?? [];
  }

  function canRemoveSelectedStep(): boolean {
    const node = asArray(state.workflowDraft.definition.nodes)
      .filter(isRecord)
      .find((item) => item.id === state.selectedStepId);
    return Boolean(node && !isLockedWorkflowNode(node));
  }

  function getFilteredWorkflows(): WorkflowDefinition[] {
    const query = ctx.normalizedSearch;

    if (!query) {
      return state.workflows;
    }

    return state.workflows.filter((workflow) =>
      [workflow.name, workflow.id ?? "", workflow.version].some((value) =>
        value.toLowerCase().includes(query),
      ),
    );
  }

  function getSubflowNames(): Map<string, string> {
    return new Map(state.workflows.flatMap((w) => (w.id != null ? [[w.id, w.name] as const] : [])));
  }

  function buildDraftGraphNodes(): GraphNodeModel[] {
    return buildGraphNodeModels(state.workflowDraft, null, getSubflowNames(), deps.getProviders());
  }

  function buildDraftGraphEdges(): GraphEdgeModel[] {
    return buildGraphEdgeModels(state.workflowDraft);
  }

  function getGraphValidationIssues(): WorkflowValidationIssue[] {
    return validateWorkflowIssues(state.workflowDraft.definition, deps.getProviders());
  }

  function getWorkflowRunWorkflow(): WorkflowDefinition | null {
    const snapshot = runWorkflowSnapshot(state.workflowRunDetail);

    if (snapshot) {
      return snapshot;
    }

    const workflowId =
      state.workflowRunDetail?.run.workflow_id ??
      state.workflowRuns.find((run) => run.id === state.selectedWorkflowRunId)?.workflow_id;

    for (const workflow of state.workflows) {
      if (workflow.id === workflowId) {
        return workflow;
      }
    }

    return null;
  }

  function ensureWorkflowNodes(): JsonRecord[] {
    if (!Array.isArray(state.workflowDraft.definition.nodes)) {
      state.workflowDraft.definition.nodes = [];
    }

    return state.workflowDraft.definition.nodes as JsonRecord[];
  }

  function getSelectedNode(): JsonRecord | null {
    return ensureWorkflowNodes().find((item) => item.id === state.selectedStepId) ?? null;
  }

  function getSelectedGraphEdge(): GraphEdgeModel | null {
    return buildDraftGraphEdges().find((edge) => edge.id === state.selectedGraphEdgeId) ?? null;
  }

  const host: WorkflowServiceHost = {
    deps,
    store,
    internal,
    get state() {
      return state;
    },
    notify,
    ctx,
    getProviders: deps.getProviders,
    getNodeKinds: deps.getNodeKinds,
    getTriggerKinds: deps.getTriggerKinds,
    getSelectedWorkflow,
    getSelectedWorkflowInputType,
    selectedWorkflowHasInputs,
    getDebugState,
    isDebugRun,
    getControlState,
    canStepWorkflowRun,
    canContinueWorkflowRun,
    canPauseWorkflowRun,
    canResumeWorkflowRun,
    canCancelWorkflowRun,
    canRequestRunInterrupt,
    getRequestableInterruptSources,
    getCurrentBreakpoints,
    canRemoveSelectedStep,
    getFilteredWorkflows,
    getSubflowNames,
    buildDraftGraphNodes,
    buildDraftGraphEdges,
    getGraphValidationIssues,
    getWorkflowRunWorkflow,
    getSelectedNode,
    getSelectedGraphEdge,
    ensureWorkflowNodes,
  };

  const runs = createWorkflowRunService(host);
  const catalogPeer: { saveSelectedWorkflowBundle: () => Promise<void> } = {
    saveSelectedWorkflowBundle: () => Promise.resolve(),
  };
  const editor = createWorkflowEditorService(host, runs, catalogPeer);
  const catalog = createWorkflowCatalogService(host, editor, runs);
  catalogPeer.saveSelectedWorkflowBundle = catalog.saveSelectedWorkflowBundle;
  const header = createWorkflowHeaderService(host, editor);

  return {
    ...store,
    catalog,
    editor,
    header,
    runs,
    internal,
    state: store,
    get workflowNodeKinds() {
      return workflowNodeKindsList();
    },
    directTransitionKeys,
    notify,
    ...catalog,
    ...editor,
    ...header,
    ...runs,
    getSelectedWorkflow,
    getSelectedWorkflowInputType,
    selectedWorkflowHasInputs,
    getFilteredWorkflows,
    getSubflowNames,
    buildDraftGraphNodes,
    buildDraftGraphEdges,
    getGraphValidationIssues,
    getWorkflowRunWorkflow,
    getSelectedNode,
    getSelectedGraphEdge,
    getDebugState,
    getControlState,
    getCursors,
    getCursorMarkers,
    getSelectedCursorId,
    getSelectedCursor,
    isSelectedCursorPaused,
    canStepWorkflowRun,
    canContinueWorkflowRun,
    canPauseWorkflowRun,
    canResumeWorkflowRun,
    canCancelWorkflowRun,
    canRequestRunInterrupt,
    getRequestableInterruptSources,
    getCurrentBreakpoints,
    isDebugRun,
    canRemoveSelectedStep,
    ensureWorkflowNodes,
  };
}

export type WorkflowServices = ReturnType<typeof createWorkflowServices>;

export type { WorkflowServicesState } from "./state";
