import type {
  CursorMarker,
  GateRecord,
  WorkflowInlineEditDescriptor,
  WorkflowSemanticHandle,
  WorkflowValidationIssue,
  WorkflowValidationSeverity,
} from "../../../core/domain/models";
import type { InterruptOrigin } from "../../../core/workflow/interrupt-regions";

/** presentation and runtime state rendered by a workflow graph node. */
export interface WorkflowNodeData {
  title: string;
  nodeId?: string;
  kind: string;
  summary?: string;
  semanticHandles?: WorkflowSemanticHandle[];
  inlineEdit?: WorkflowInlineEditDescriptor | null;
  validationCount?: number;
  validationSeverity?: WorkflowValidationSeverity;
  validationIssues?: WorkflowValidationIssue[];
  statusLabel?: string;
  executionCount?: number;
  approvalPrompt?: string;
  inputPrompt?: string;
  running?: boolean;
  status?: string;
  protected?: boolean;
  locked?: boolean;
  skipped?: boolean;
  readOnly?: boolean;
  allowGateResolution?: boolean;
  gate?: GateRecord | null;
  debugBreakpoint?: boolean;
  /** threads of control standing on this node; a node may carry several. */
  cursors?: CursorMarker[];
  /** the interrupt handler region this node belongs to, or null for the main flow. */
  interruptRegion?: InterruptOrigin | null;
  /** true when this node is the region's declared entry. */
  interruptEntry?: boolean;
}
