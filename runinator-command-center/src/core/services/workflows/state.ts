import type {
  GateRecord,
  JsonRecord,
  RunSummary,
  WorkflowDefinition,
  WorkflowLayoutDirection,
  WorkflowRunDetail,
  WorkflowTrigger,
} from "../../domain/models";
import {
  createStepEditorState,
  emptyWorkflowTriggerDraft,
  newWorkflowDraft,
} from "../../workflow/editor-defaults";

export type StepEditorState = ReturnType<typeof createStepEditorState>;

export interface WorkflowServicesState {
  workflows: WorkflowDefinition[];
  selectedWorkflowId: string | null;
  workflowDraft: WorkflowDefinition;
  workflowJson: string;
  workflowWdl: string;
  workflowWdlError: string;
  workflowConcurrency: number;
  workflowSettingsOpen: boolean;
  runInputOpen: boolean;
  runInputDraft: JsonRecord;
  runInputDebug: boolean;
  workflowTriggers: WorkflowTrigger[];
  triggerEditorOpen: boolean;
  triggerEditorCreating: boolean;
  triggerEditorError: string;
  triggerDraft: WorkflowTrigger;
  triggerJson: { configuration: string; metadata: string };
  workflowEditorMode: "graph" | "json" | "wdl";
  workflowLayoutDirection: WorkflowLayoutDirection;
  workflowInspectorMode: "step";
  stepEditorOpen: boolean;
  stepEditorCreating: boolean;
  stepEditorCreatedNodeId: string;
  stepEditorError: string;
  stepEditor: StepEditorState;
  workflowRuns: RunSummary[];
  workflowLayoutVersion: number;
  selectedWorkflowRunId: string | null;
  workflowRunDetail: WorkflowRunDetail | null;
  openRunIds: string[];
  workflowRunGates: GateRecord[];
  workflowRunGateRunId: string | null;
  workflowRunGateFingerprint: string;
  workflowNodeDetailExtra: string;
  selectedStepId: string;
  inlineEditNodeId: string;
  selectedGraphEdgeId: string;
  selectedWorkflowRunNodeId: string;
  selectedWorkflowNodeRunId: string | null;
  isDirty: boolean;
  watchExpressionsByWorkflowId: Record<string, string[]>;
}

export interface WorkflowServicesInternal {
  runDetailById: Map<string, WorkflowRunDetail | null>;
  latestWorkflowRunPushVersion: Map<string, number>;
  latestWorkflowRunHttpRequest: Map<string, number>;
  nextWorkflowRunDetailVersion: number;
  nextWorkflowRunHttpRequestId: number;
  nextWorkflowRunGateRequestId: number;
  nextBreakpointMutationId: number;
  pendingBreakpointPatch: { runId: string; breakpoints: string[]; mutationId: number } | null;
  workflowWdlSyncTimer: ReturnType<typeof setTimeout> | null;
  workflowJsonWriteReleaseTimer: ReturnType<typeof setTimeout> | null;
  workflowWdlWriteReleaseTimer: ReturnType<typeof setTimeout> | null;
  stepEditorApplyTimer: ReturnType<typeof setTimeout> | null;
  workflowJsonWriteGuard: boolean;
  workflowWdlWriteGuard: boolean;
  stepEditorHydrating: boolean;
  stepEditorBaselineDefinition: JsonRecord | null;
}

export function createWorkflowServicesInternal(): WorkflowServicesInternal {
  return {
    runDetailById: new Map(),
    latestWorkflowRunPushVersion: new Map(),
    latestWorkflowRunHttpRequest: new Map(),
    nextWorkflowRunDetailVersion: 0,
    nextWorkflowRunHttpRequestId: 0,
    nextWorkflowRunGateRequestId: 0,
    nextBreakpointMutationId: 0,
    pendingBreakpointPatch: null,
    workflowWdlSyncTimer: null,
    workflowJsonWriteReleaseTimer: null,
    workflowWdlWriteReleaseTimer: null,
    stepEditorApplyTimer: null,
    workflowJsonWriteGuard: false,
    workflowWdlWriteGuard: false,
    stepEditorHydrating: false,
    stepEditorBaselineDefinition: null,
  };
}

export function createWorkflowServicesState(): WorkflowServicesState {
  return {
    workflows: [],
    selectedWorkflowId: null,
    workflowDraft: newWorkflowDraft(),
    workflowJson: "{}",
    workflowWdl: "",
    workflowWdlError: "",
    workflowConcurrency: 1,
    workflowSettingsOpen: false,
    runInputOpen: false,
    runInputDraft: {},
    runInputDebug: false,
    workflowTriggers: [],
    triggerEditorOpen: false,
    triggerEditorCreating: false,
    triggerEditorError: "",
    triggerDraft: emptyWorkflowTriggerDraft("", "cron"),
    triggerJson: { configuration: "{}", metadata: "{}" },
    workflowEditorMode: "graph",
    workflowLayoutDirection: "horizontal",
    workflowInspectorMode: "step",
    stepEditorOpen: false,
    stepEditorCreating: false,
    stepEditorCreatedNodeId: "",
    stepEditorError: "",
    stepEditor: createStepEditorState(),
    workflowRuns: [],
    workflowLayoutVersion: 0,
    selectedWorkflowRunId: null,
    workflowRunDetail: null,
    openRunIds: [],
    workflowRunGates: [],
    workflowRunGateRunId: null,
    workflowRunGateFingerprint: "",
    workflowNodeDetailExtra: "",
    selectedStepId: "",
    inlineEditNodeId: "",
    selectedGraphEdgeId: "",
    selectedWorkflowRunNodeId: "",
    selectedWorkflowNodeRunId: null,
    isDirty: false,
    watchExpressionsByWorkflowId: {},
  };
}
