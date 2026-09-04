import type { JsonRecord } from "../../json";

// what happens to downstream links when a member workflow fails. authoring-only: it seeds the `on`
// selector of newly drawn links (`halt` -> on success, `continue` -> on complete).
export type PipelineFailurePolicy = "halt" | "continue";

// What happens to the pipeline run when a graph member workflow fails.
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
  workspace?: JsonRecord | null;
  on_step_failure: PipelineFailurePolicy;
  links_enabled_by_default: boolean;
  default_parameters: JsonRecord;
  max_chain_depth: number | null;
  // the failure mode copied onto newly-added/imported members.
  default_failure_mode: PipelineMemberFailureMode;
}

export type PipelineJoinMode = "all" | "any" | "first_success";
export type PipelineLinkSelector = "success" | "failure" | "complete";

export interface PipelineMember {
  workspace?: JsonRecord | null;
  key: string;
  workflow_id: string;
  failure_mode: PipelineMemberFailureMode;
}

export interface PipelineLink {
  id: string;
  from: string;
  to: string;
  on: PipelineLinkSelector;
  enabled: boolean;
  parameters: JsonRecord;
}

export interface PipelineJoin {
  target: string;
  mode: PipelineJoinMode;
  parameters: JsonRecord;
}

export interface PipelineGraph {
  version: number;
  members: PipelineMember[];
  links: PipelineLink[];
  joins: Record<string, PipelineJoin>;
}

export interface PipelineConcurrency {
  max_concurrent_runs: number;
  on_conflict: "allow" | "skip" | "queue" | "cancel_previous";
}

// A named, versioned first-class pipeline DAG plus its authoring and concurrency settings.
export interface Pipeline {
  id: string | null;
  name: string;
  // Stable logical key; display-name edits and namespace moves preserve it.
  key?: string | null;
  // Required namespace prefix for the canonical pipeline path.
  namespace?: string | null;
  description: string | null;
  // owning organization (tenant); null = platform-global. server-managed (stamped on create).
  org_id?: string | null;
  // Disabled pipelines reject manual, trigger, and ingress starts.
  enabled: boolean;
  graph: PipelineGraph;
  concurrency: PipelineConcurrency;
  defaults: PipelineDefaults;
  metadata: JsonRecord;
  created_at?: string | null;
  updated_at?: string | null;
}

export function pipelinePath(pipeline: Pick<Pipeline, "name" | "key" | "namespace">): string {
  const key = pipeline.key ?? pipeline.name;
  return pipeline.namespace ? `${pipeline.namespace}.${key}` : key;
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
