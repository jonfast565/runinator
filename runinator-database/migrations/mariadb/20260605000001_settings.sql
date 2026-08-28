-- unified config/secret store. `value` holds ciphertext (the web service encrypts before writing);
-- `updated_at` is unix seconds, used for import reconciliation.
CREATE TABLE IF NOT EXISTS settings (
    kind VARCHAR(64) NOT NULL,
    scope VARCHAR(255) NOT NULL,
    name VARCHAR(255) NOT NULL,
    value LONGBLOB NOT NULL,
    updated_at BIGINT NOT NULL,
    PRIMARY KEY (kind, scope, name)
);
