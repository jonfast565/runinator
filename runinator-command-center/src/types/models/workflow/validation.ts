export type WorkflowValidationSeverity = "error" | "warning";

export interface WorkflowValidationIssue {
  severity: WorkflowValidationSeverity;
  message: string;
  nodeId: string;
  edgeKey?: string;
}

export interface WorkflowInlineEditDescriptor {
  label: string;
  value: string;
  valueKind: "text" | "number";
}
