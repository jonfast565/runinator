CREATE INDEX IF NOT EXISTS idx_adapter_deliveries_adapter_received
    ON adapter_deliveries(adapter_id, received_at, id);
CREATE INDEX IF NOT EXISTS idx_adapter_deliveries_adapter_state_received
    ON adapter_deliveries(adapter_id, state, received_at, id);
CREATE INDEX IF NOT EXISTS idx_adapter_poll_dispatches_adapter_created
    ON orchestration_adapter_poll_dispatches(adapter_id, created_at, id);
