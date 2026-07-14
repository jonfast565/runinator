import type {
  ControlFrame,
  DebugFrame,
  JsonRecord,
  ProviderMetadata,
  RuninatorType,
  WorkflowDefinition,
  WorkflowNodeKindMetadata,
  WorkflowTriggerKindMetadata,
  WorkflowValidationIssue,
} from "../../domain/models";
import type { GraphEdgeModel, GraphNodeModel } from "../../workflow/graph-model";
import type { AppTab } from "../../navigation/app";
import type { AppService } from "../app";
import { createStore } from "../event-bus";
import type { WorkflowServicesInternal, WorkflowServicesState } from "./state";

export interface WorkflowServiceDeps {
  app: AppService;
  getProviders: () => ProviderMetadata[];
  getNodeKinds: () => WorkflowNodeKindMetadata[];
  getTriggerKinds: () => WorkflowTriggerKindMetadata[];
  refreshResources?(): void;
  confirm?(message: string): boolean;
  downloadBlob?(fileName: string, blob: Blob): void;
  downloadTextFile?(fileName: string, contents: string, mimeType?: string): void;
}

export interface WorkflowServiceHost {
  deps: Required<WorkflowServiceDeps>;
  store: ReturnType<typeof createStore<WorkflowServicesState>>;
  internal: WorkflowServicesInternal;
  state: WorkflowServicesState;
  notify(): void;
  ctx: {
    runOperation: <T>(
      label: string,
      operation: () => Promise<T>,
      options?: { silent?: boolean },
    ) => Promise<T>;
    setStatus: (text: string) => void;
    setError: (text: string) => void;
    readonly normalizedSearch: string;
    activeTab: AppTab;
  };
  getProviders: () => ProviderMetadata[];
  getNodeKinds: () => WorkflowNodeKindMetadata[];
  getTriggerKinds: () => WorkflowTriggerKindMetadata[];
  getSelectedWorkflow(): WorkflowDefinition | null;
  getSelectedWorkflowInputType(): RuninatorType | null;
  selectedWorkflowHasInputs(): boolean;
  getDebugState(): DebugFrame | null;
  isDebugRun(): boolean;
  getControlState(): ControlFrame | null;
  canStepWorkflowRun(): boolean;
  canContinueWorkflowRun(): boolean;
  canPauseWorkflowRun(): boolean;
  canResumeWorkflowRun(): boolean;
  canCancelWorkflowRun(): boolean;
  getCurrentBreakpoints(): string[];
  canRemoveSelectedStep(): boolean;
  getFilteredWorkflows(): WorkflowDefinition[];
  getSubflowNames(): Map<string, string>;
  buildDraftGraphNodes(): GraphNodeModel[];
  buildDraftGraphEdges(): GraphEdgeModel[];
  getGraphValidationIssues(): WorkflowValidationIssue[];
  getWorkflowRunWorkflow(): WorkflowDefinition | null;
  getSelectedNode(): JsonRecord | null;
  getSelectedGraphEdge(): GraphEdgeModel | null;
  ensureWorkflowNodes(): JsonRecord[];
}
