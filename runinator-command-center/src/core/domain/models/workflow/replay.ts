import type { WorkflowDefinition } from "./definition";
import type { JsonValue } from "../../json";

export interface ReplayPlan {
  source_run_id: string;
  from_step_id: string | null;
  workflow_snapshot: WorkflowDefinition | null;
  seeded_receipts: { node_id: string; effect_id: string; attempt: number }[];
  actions: {
    node_id: string;
    provider: string;
    function: string;
    declared_idempotency_key: JsonValue | null;
    previous_resolved_idempotency_keys: JsonValue[];
    reason: string;
  }[];
  reasons: string[];
  verdict: "safe" | "review" | "blocked";
  plan_fingerprint: string;
}

export interface WorkflowContractImpact {
  compatibility: "unchanged" | "compatible" | "breaking";
  reasons: string[];
  previous_version: string | null;
  proposed_version: string;
  requires_major_bump: boolean;
  dependents: { kind: string; id: string; name: string; pinned: boolean }[];
}
