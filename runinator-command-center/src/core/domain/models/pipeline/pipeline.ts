import type { JsonRecord } from "../../json";

// what happens to downstream links when a member workflow fails. authoring-only: it seeds the `on`
// selector of newly drawn links (`halt` -> on success, `continue` -> on complete).
export type PipelineFailurePolicy = "halt" | "continue";

// what happens to the *pipeline run* when a member workflow fails, evaluated per member (an
// override in `Pipeline.member_failure_modes`, falling back to `PipelineDefaults.default_failure_mode`).
// named after PowerShell's $ErrorActionPreference, which this mirrors one-for-one:
//   - stop: the failed member fires none of its outgoing links; the run still settles once every
//     already-started member quiesces, and this failure counts toward that settlement.
//   - continue: the failed member's outgoing links still fire per their own `on` selector (today's
//     behavior), and this failure counts toward settlement. the default.
//   - silently_continue: like continue, but this member's failure alone does not fail the run.
//   - inquire: the failed member fires no outgoing links until a human resolves the run's pending
//     inquiry (continue or abort); the run pauses (`approval_required`) rather than settling.
export type PipelineMemberFailureMode = "stop" | "continue" | "silently_continue" | "inquire";

export interface PipelineDefaults {
  on_step_failure: PipelineFailurePolicy;
  links_enabled_by_default: boolean;
  default_parameters: JsonRecord;
  max_chain_depth: number | null;
  // the failure mode applied to a member with no entry in `Pipeline.member_failure_modes`.
  default_failure_mode: PipelineMemberFailureMode;
}

// a named pipeline instance: a chosen set of member workflows plus authoring defaults. links
// between members stay `chained` workflow triggers stamped with this pipeline's id.
export interface Pipeline {
  id: string | null;
  name: string;
  description: string | null;
  // owning organization (tenant); null = platform-global. server-managed (stamped on create).
  org_id?: string | null;
  workflow_ids: string[];
  // per-member override of defaults.default_failure_mode, keyed by member workflow id. a member
  // absent from this map uses the pipeline's default.
  member_failure_modes: Record<string, PipelineMemberFailureMode>;
  defaults: PipelineDefaults;
  metadata: JsonRecord;
  created_at?: string | null;
  updated_at?: string | null;
}

export function defaultPipelineDefaults(): PipelineDefaults {
  return {
    on_step_failure: "halt",
    links_enabled_by_default: true,
    default_parameters: {},
    max_chain_depth: null,
    default_failure_mode: "continue",
  };
}
