import type { JsonRecord } from "../../json";
import type { RuninatorType } from "../provider/runinator-type";

export interface WorkflowDefinition {
  id: string | null;
  name: string;
  // semantic version string, e.g. "1.2.0".
  version: string;
  enabled: boolean;
  input_type: JsonRecord;
  definition: JsonRecord;
  // owning organization (tenant); null means platform-global / unassigned.
  org_id?: string | null;
}

/** read the workflow input schema as a RuninatorType when present and well-formed. */
export function workflowInputType(workflow: WorkflowDefinition): RuninatorType | null {
  const type = workflow.input_type.type;
  return typeof type === "string" ? (workflow.input_type as RuninatorType) : null;
}
