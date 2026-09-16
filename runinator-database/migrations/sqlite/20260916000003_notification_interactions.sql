ALTER TABLE notification_policies ADD COLUMN provider TEXT NULL;
ALTER TABLE notification_policies ADD COLUMN provider_function TEXT NULL;
ALTER TABLE notification_policies ADD COLUMN interactive INTEGER NOT NULL DEFAULT 0;
ALTER TABLE notification_deliveries ADD COLUMN provider TEXT NULL;
ALTER TABLE notification_deliveries ADD COLUMN provider_function TEXT NULL;

CREATE TABLE notification_interactions (
    id BLOB PRIMARY KEY NOT NULL,
    notification_id BLOB NOT NULL UNIQUE,
    org_id BLOB NULL,
    target_json TEXT NOT NULL,
    actions_json TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'open',
    resolved_action TEXT NULL,
    resolved_by BLOB NULL,
    resolved_at INTEGER NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY(notification_id) REFERENCES notifications(id) ON DELETE CASCADE,
    FOREIGN KEY(resolved_by) REFERENCES users(id) ON DELETE SET NULL
);

CREATE TABLE notification_conversations (
    id BLOB PRIMARY KEY NOT NULL,
    interaction_id BLOB NOT NULL,
    delivery_id BLOB NOT NULL,
    org_scope TEXT NOT NULL,
    source TEXT NOT NULL,
    scope TEXT NOT NULL,
    correlation_key TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    UNIQUE(org_scope, source, scope, correlation_key),
    FOREIGN KEY(interaction_id) REFERENCES notification_interactions(id) ON DELETE CASCADE,
    FOREIGN KEY(delivery_id) REFERENCES notification_deliveries(id) ON DELETE CASCADE
);

CREATE INDEX idx_notification_conversations_interaction
    ON notification_conversations(interaction_id, created_at);
CREATE INDEX idx_notification_conversations_delivery
    ON notification_conversations(delivery_id);
CREATE INDEX idx_notification_interactions_resolved_by
    ON notification_interactions(resolved_by);
