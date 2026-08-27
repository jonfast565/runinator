export type WorkflowValidationSeverity = "error" | "warning";

export interface WorkflowValidationIssue {
  severity: WorkflowValidationSeverity;
  message: string;
  nodeId: string;
  edgeKey?: string;
  /**
   * The interrupt declaration that owns this problem, when the issue came from an interrupt
   * handler validation pass.  It lets the authoring UI keep a handler's repairs with its card
   * instead of trying to infer ownership from display text.
   */
  interruptHandlerId?: string;
}

export interface WorkflowInlineEditDescriptor {
  label: string;
  value: string;
  valueKind: "text" | "number";
}
