import type {
  WorkflowConnectionHandle,
  WorkflowDirectTransitionKey,
} from "./transitions";
import type { WorkflowValidationSeverity } from "./validation";

export type WorkflowEditorEdgeKind = "direct" | "branch" | "control";
export type WorkflowEdgeStyle = "bezier" | "straight" | "square";

export interface WorkflowEdgeSemanticOption {
  id: string;
  label: string;
  description: string;
}

export interface WorkflowSemanticHandle {
  id: string;
  label: string;
  type: "source" | "target";
  semanticOptionId?: string;
}

export type WorkflowEdgeEditorMatchKind = "equals" | "not_equals" | "exists" | "when";

export interface WorkflowEdgeEditorDraft {
  edgeId: string;
  source: string;
  target: string;
  optionId: string;
  sourceHandle?: WorkflowConnectionHandle | null;
  targetHandle?: WorkflowConnectionHandle | null;
  edgeStyle: WorkflowEdgeStyle;
  labelAnchor: number;
  label: string;
  whenJson: string;
  matchKind: WorkflowEdgeEditorMatchKind;
  matchJson: string;
  canEditLabel: boolean;
  canEditCondition: boolean;
  canEditSwitchCase: boolean;
  canMove: boolean;
  orderIndex: number;
  orderCount: number;
  // selection priority for predicate edges; lower numbers are evaluated first. null means unset.
  priority: number | null;
  canEditPriority: boolean;
}

export interface WorkflowEdgeLabelOffset {
  x: number;
  y: number;
}

export interface WorkflowEdgeLabelAnchor {
  position: number;
}

export interface WorkflowEditorEdgeData {
  kind: WorkflowEditorEdgeKind;
  transitionKey?: WorkflowDirectTransitionKey;
  branchIndex?: number;
  parameterKey?: string;
  parameterIndex?: number;
  sourceHandle?: WorkflowConnectionHandle;
  targetHandle?: WorkflowConnectionHandle;
  edgeStyle?: WorkflowEdgeStyle;
  labelOffset?: WorkflowEdgeLabelOffset | null;
  labelAnchor?: WorkflowEdgeLabelAnchor | null;
  parallelOffset?: number;
  validationCount?: number;
  validationSeverity?: WorkflowValidationSeverity;
  validationMessages?: string[];
  editable: boolean;
}
