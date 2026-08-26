import type { JsonRecord } from "../../json";
import type { RuninatorType } from "../provider/runinator-type";

export interface WorkflowDefinition {
  id: string | null;
  name: string;
  // stable logical key; display-name edits and namespace moves preserve it.
  key?: string | null;
  // durable identity is the UUID; namespace is the mutable human-facing path prefix.
  namespace?: string | null;
  // semantic version string, e.g. "1.2.0".
  version: string;
  enabled: boolean;
  input_type: JsonRecord;
  definition: JsonRecord;
  // owning organization (tenant); null means platform-global / unassigned.
  org_id?: string | null;
}

/** The canonical, name-first path shown in navigation and inspectors. */
export function workflowPath(workflow: Pick<WorkflowDefinition, "name" | "key" | "namespace">): string {
  const key = workflow.key ?? workflow.name;
  return workflow.namespace ? `${workflow.namespace}.${key}` : key;
}

/** read the workflow input schema as a RuninatorType when present and well-formed. */
export function workflowInputType(workflow: WorkflowDefinition): RuninatorType | null {
  const type = workflow.input_type.type;
  return typeof type === "string" ? (workflow.input_type as RuninatorType) : null;
}
