ALTER TABLE notification_policies ADD COLUMN provider VARCHAR(255) NULL;
ALTER TABLE notification_policies ADD COLUMN provider_function VARCHAR(255) NULL;
ALTER TABLE notification_policies ADD COLUMN interactive BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE notification_deliveries ADD COLUMN provider VARCHAR(255) NULL;
ALTER TABLE notification_deliveries ADD COLUMN provider_function VARCHAR(255) NULL;

CREATE TABLE notification_interactions (
    id BINARY(16) PRIMARY KEY NOT NULL,
    notification_id BINARY(16) NOT NULL UNIQUE,
    org_id BINARY(16) NULL,
    target_json LONGTEXT NOT NULL,
    actions_json LONGTEXT NOT NULL,
    state VARCHAR(32) NOT NULL DEFAULT 'open',
    resolved_action VARCHAR(255) NULL,
    resolved_by BINARY(16) NULL,
    resolved_at BIGINT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    KEY idx_notification_interactions_resolved_by(resolved_by),
    CONSTRAINT fk_notification_interactions_notification FOREIGN KEY(notification_id) REFERENCES notifications(id) ON DELETE CASCADE,
    CONSTRAINT fk_notification_interactions_user FOREIGN KEY(resolved_by) REFERENCES users(id) ON DELETE SET NULL
);

CREATE TABLE notification_conversations (
    id BINARY(16) PRIMARY KEY NOT NULL,
    interaction_id BINARY(16) NOT NULL,
    delivery_id BINARY(16) NOT NULL,
    org_scope VARCHAR(255) NOT NULL,
    source VARCHAR(255) NOT NULL,
    scope VARCHAR(255) NOT NULL,
    correlation_key VARCHAR(255) NOT NULL,
    created_at BIGINT NOT NULL,
    UNIQUE KEY uq_notification_conversation(org_scope, source, scope, correlation_key),
    KEY idx_notification_conversations_interaction(interaction_id, created_at),
    KEY idx_notification_conversations_delivery(delivery_id),
    CONSTRAINT fk_notification_conversations_interaction FOREIGN KEY(interaction_id) REFERENCES notification_interactions(id) ON DELETE CASCADE,
    CONSTRAINT fk_notification_conversations_delivery FOREIGN KEY(delivery_id) REFERENCES notification_deliveries(id) ON DELETE CASCADE
);
