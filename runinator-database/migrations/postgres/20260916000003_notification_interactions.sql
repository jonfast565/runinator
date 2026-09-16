ALTER TABLE notification_policies ADD COLUMN provider VARCHAR(255) NULL;
ALTER TABLE notification_policies ADD COLUMN provider_function VARCHAR(255) NULL;
ALTER TABLE notification_policies ADD COLUMN interactive BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE notification_deliveries ADD COLUMN provider VARCHAR(255) NULL;
ALTER TABLE notification_deliveries ADD COLUMN provider_function VARCHAR(255) NULL;

CREATE TABLE notification_interactions (
    id UUID PRIMARY KEY NOT NULL,
    notification_id UUID NOT NULL UNIQUE REFERENCES notifications(id) ON DELETE CASCADE,
    org_id UUID NULL,
    target_json TEXT NOT NULL,
    actions_json TEXT NOT NULL,
    state VARCHAR(32) NOT NULL DEFAULT 'open',
    resolved_action VARCHAR(255) NULL,
    resolved_by UUID NULL REFERENCES users(id) ON DELETE SET NULL,
    resolved_at BIGINT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE TABLE notification_conversations (
    id UUID PRIMARY KEY NOT NULL,
    interaction_id UUID NOT NULL REFERENCES notification_interactions(id) ON DELETE CASCADE,
    delivery_id UUID NOT NULL REFERENCES notification_deliveries(id) ON DELETE CASCADE,
    org_scope VARCHAR(255) NOT NULL,
    source VARCHAR(255) NOT NULL,
    scope VARCHAR(255) NOT NULL,
    correlation_key VARCHAR(255) NOT NULL,
    created_at BIGINT NOT NULL,
    UNIQUE(org_scope, source, scope, correlation_key)
);

CREATE INDEX idx_notification_conversations_interaction
    ON notification_conversations(interaction_id, created_at);
CREATE INDEX idx_notification_conversations_delivery
    ON notification_conversations(delivery_id);
CREATE INDEX idx_notification_interactions_resolved_by
    ON notification_interactions(resolved_by);
