import type {
  ControlFrame,
  DebugFrame,
  JsonRecord,
  ProviderMetadata,
  RuninatorType,
  WorkflowDefinition,
  WorkflowValidationIssue,
} from "../../domain/models";
import { runWorkflowSnapshot, workflowInputType } from "../../domain/models";
import { coerceControlFrame, coerceDebugFrame } from "../../domain/models/workflow-state";
import {
  asArray,
  buildGraphEdgeModels,
  buildGraphNodeModels,
  directTransitionKeys,
  isRecord,
  validateWorkflowIssues,
  workflowNodeKindsList,
} from "../../workflow/index";
import type { GraphEdgeModel, GraphNodeModel } from "../../workflow/graph-model";
import type { AppTab } from "../../navigation/app";
import { isLockedWorkflowNode } from "../../workflow/editor-defaults";
import { createStore } from "../event-bus";
import { createWorkflowCatalogService } from "./catalog";
import { createWorkflowEditorService } from "./editor";
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
    runOperation: <T>(label: string, operation: () => Promise<T>, options?: { silent?: boolean }) =>
      deps.app.runOperation(label, operation, options),
    setStatus: (text: string) => {
      deps.app.setStatus(text);
    },
    setError: (text: string) => {
      deps.app.setError(text);
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

  function canStepWorkflowRun(): boolean {
    return state.workflowRunDetail?.run.status === "debug_paused";
  }

  function canContinueWorkflowRun(): boolean {
    return state.workflowRunDetail?.run.status === "debug_paused";
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
    saveSelectedWorkflowBundle: async () => undefined,
  };
  const editor = createWorkflowEditorService(host, runs, catalogPeer);
  const catalog = createWorkflowCatalogService(host, editor, runs);
  catalogPeer.saveSelectedWorkflowBundle = catalog.saveSelectedWorkflowBundle;

  return {
    ...store,
    catalog,
    editor,
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
    canStepWorkflowRun,
    canContinueWorkflowRun,
    canPauseWorkflowRun,
    canResumeWorkflowRun,
    canCancelWorkflowRun,
    getCurrentBreakpoints,
    isDebugRun,
    canRemoveSelectedStep,
    ensureWorkflowNodes,
  };
}

export type WorkflowServices = ReturnType<typeof createWorkflowServices>;

export type { WorkflowServicesState } from "./state";
