# Persistence schema invariants

The greenfield Runinator schema contains 79 application tables in each SQL dialect. The SQLite
schema tests pin that budget and the normalization rules below so derived storage cannot quietly
return.

## Consolidated ownership

- `workflow_runs` owns both run lifecycle data and the workflow VM execution-state payload.
- `organizations` owns its quota limits and quota metadata.
- `role_assignments` is the sole membership/role relation. Its candidate key is
  `(principal_kind, principal_id, scope_key, role)`; scope type and identifier are derived from the
  canonical `scope_key`.
- `function_aliases` identifies a function version through `version_id`; the human version
  string is derived from `function_versions`.
- `workflow_effects` derives its run through `continuation_id`, and effect output events derive both
  continuation and run through `effect_id`.
- `ingress_admissions` uses `org_scope` as its canonical optional-organization key, has exactly one
  typed target (`workflow_id` or `pipeline_id`), and enforces the target with a foreign key.
- `orchestration_bindings` derives organization, scope, correlation key, and pipeline from its
  admission. Correlation aliases derive generation from their binding.

## Important candidate keys

- ingress admission: `(org_scope, scope, correlation_key)`
- orchestration binding generation: `(admission_id, generation)`
- correlated pipeline run: `(orchestration_binding_id, execution_epoch)`
- workflow effect: `(continuation_id, sequence, attempt)` and `idempotency_key`
- workflow output event: `(effect_id, attempt)`

The removed legacy relations are `runs`, `run_chunks`, `run_artifacts`, `org_memberships`,
`team_members`, `org_quotas`, and `workflow_run_execution_states`.
