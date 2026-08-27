ALTER TABLE orchestration_bindings ADD COLUMN adapter_id BLOB NULL;
ALTER TABLE orchestration_bindings ADD COLUMN adapter_revision INTEGER NULL;
CREATE INDEX IF NOT EXISTS idx_orchestration_bindings_adapter
    ON orchestration_bindings(adapter_id, adapter_revision);

ALTER TABLE external_operations ADD COLUMN epoch INTEGER NOT NULL DEFAULT 0;
ALTER TABLE external_operations ADD COLUMN workflow_run_id BLOB NULL;
ALTER TABLE external_operations ADD COLUMN effect_id BLOB NULL;
CREATE UNIQUE INDEX IF NOT EXISTS idx_external_operations_effect
    ON external_operations(effect_id);
