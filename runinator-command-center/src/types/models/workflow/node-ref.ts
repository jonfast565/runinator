export type WorkflowNodeId = string;

export interface WorkflowNodeRef {
  $node: WorkflowNodeId;
}

export type WorkflowPathSegment = string | number;
