import type { JsonRecord } from "../../json";

/** where an accepted definition came from. `pack` is the one that matters most: a
 * `workflows apply` overwrites definitions wholesale, so it is the change most worth undoing. */
export type RevisionSource = "ui" | "pack" | "api" | "duplicate" | "rollback";

/** one immutable capture of a workflow definition. `revision` is the handle a rollback names. */
export interface WorkflowRevision {
  id: string;
  workflow_id: string;
  revision: number;
  // semantic version string, e.g. "1.2.0".
  version: string;
  name: string;
  input_type: JsonRecord;
  output_type?: JsonRecord;
  digest?: string;
  definition: JsonRecord;
  source: RevisionSource;
  actor_id?: string | null;
  actor_kind: string;
  note?: string | null;
  created_at?: string | null;
}

/** an unattributed write shows its kind rather than a blank cell. */
export function revisionAuthorLabel(revision: WorkflowRevision): string {
  return revision.actor_id ? `${revision.actor_kind} · ${revision.actor_id}` : revision.actor_kind;
}
