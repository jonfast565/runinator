ALTER TABLE orchestration_bindings ADD COLUMN adapter_id UUID NULL REFERENCES orchestration_adapters(id);
ALTER TABLE orchestration_bindings ADD COLUMN adapter_revision BIGINT NULL;
CREATE INDEX idx_orchestration_bindings_adapter
    ON orchestration_bindings(adapter_id, adapter_revision);

ALTER TABLE external_operations ADD COLUMN epoch BIGINT NOT NULL DEFAULT 0;
ALTER TABLE external_operations ADD COLUMN workflow_run_id UUID NULL;
ALTER TABLE external_operations ADD COLUMN effect_id UUID NULL;
CREATE UNIQUE INDEX idx_external_operations_effect ON external_operations(effect_id);
