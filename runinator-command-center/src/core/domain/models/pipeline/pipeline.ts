import type { JsonRecord } from "../../json";

// what happens to downstream links when a member workflow fails. authoring-only: it seeds the `on`
// selector of newly drawn links (`halt` -> on success, `continue` -> on complete).
export type PipelineFailurePolicy = "halt" | "continue";

export interface PipelineDefaults {
  on_step_failure: PipelineFailurePolicy;
  links_enabled_by_default: boolean;
  default_parameters: JsonRecord;
  max_chain_depth: number | null;
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
  };
}
