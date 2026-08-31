-- Retention scans added after the correlated-orchestration and file-history tables landed.
CREATE INDEX IF NOT EXISTS idx_pipeline_member_attempts_archive
    ON pipeline_member_attempts(created_at, pipeline_run_id, id);
CREATE INDEX IF NOT EXISTS idx_workflow_files_archive
    ON workflow_files(created_at, scope, workflow_run_id, id);
CREATE INDEX IF NOT EXISTS idx_ingress_admissions_archive
    ON ingress_admissions(status, created_at, id);
CREATE INDEX IF NOT EXISTS idx_ingress_events_archive
    ON ingress_events(received_at, admission_id, id);
CREATE INDEX IF NOT EXISTS idx_orchestration_bindings_archive
    ON orchestration_bindings(status, created_at, id);
CREATE INDEX IF NOT EXISTS idx_orchestration_epochs_archive
    ON orchestration_epochs(created_at, binding_id, id);
CREATE INDEX IF NOT EXISTS idx_orchestration_reductions_archive
    ON orchestration_event_reductions(created_at, binding_id, id);
CREATE INDEX IF NOT EXISTS idx_orchestration_intents_archive
    ON orchestration_pending_intents(created_at, binding_id, id);
CREATE INDEX IF NOT EXISTS idx_orchestration_commands_archive
    ON orchestration_commands(status, created_at, binding_id, id);
CREATE INDEX IF NOT EXISTS idx_orchestration_evidence_archive
    ON orchestration_evidence(created_at, binding_id, id);
CREATE INDEX IF NOT EXISTS idx_external_operations_archive
    ON external_operations(status, created_at, binding_id, id);
CREATE INDEX IF NOT EXISTS idx_workspace_leases_archive
    ON workspace_leases(status, created_at, admission_id, id);
CREATE INDEX IF NOT EXISTS idx_workflow_mutexes_archive
    ON workflow_mutexes(holder_run_id, updated_at, name);
