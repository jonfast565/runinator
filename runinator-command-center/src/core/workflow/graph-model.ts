import type {
  WorkflowEditorEdgeData,
  WorkflowInlineEditDescriptor,
  WorkflowSemanticHandle,
  WorkflowValidationIssue,
  WorkflowValidationSeverity,
} from "../domain/models";

export interface GraphNodeData {
  title: string;
  nodeId: string;
  kind: string;
  summary: string;
  semanticHandles: WorkflowSemanticHandle[];
  inlineEdit: WorkflowInlineEditDescriptor | null;
  validationIssues: WorkflowValidationIssue[];
  validationCount: number;
  validationSeverity?: WorkflowValidationSeverity;
  statusLabel?: string;
  executionCount: number;
  approvalPrompt?: string;
  inputPrompt?: string;
  running: boolean;
  status?: string;
  protected: boolean;
  locked: boolean;
  skipped: boolean;
  debugBreakpoint: boolean;
}

export interface GraphNodeModel {
  id: string;
  type: string;
  position: { x: number; y: number };
  data: GraphNodeData;
  class?: string;
}

export interface GraphEdgePathOptions {
  offset?: number;
  borderRadius?: number;
}

/** Fields read by portable edge editor helpers; compatible with Vue Flow edges. */
export interface GraphEdgeLike {
  id?: string;
  source: string;
  target: string;
  sourceHandle?: string | null;
  targetHandle?: string | null;
  data?: WorkflowEditorEdgeData;
}

export interface GraphEdgeModel extends GraphEdgeLike {
  id: string;
  type: string;
  label?: string;
  data: WorkflowEditorEdgeData;
  updatable?: boolean | string;
  interactionWidth?: number;
  pathOptions?: GraphEdgePathOptions;
  zIndex?: number;
  animated?: boolean;
}
