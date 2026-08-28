-- turn the manual idempotency store into a claimable one so an action node declaring
-- `.idempotent(key: ...)` can reserve its key before invoking a provider and record the outcome
-- against it. `owner_node_run_id` is the node run holding an unfinished claim; `claimed_at` dates that
-- reservation so one abandoned by a crashed worker becomes takeable instead of blocking the key
-- forever; `completed_at` marks the row as a replayable result rather than a reservation. all three
-- are null for the manual put/get store, which keeps writing first-writer-wins rows with no owner.
ALTER TABLE idempotency_keys ADD COLUMN owner_node_run_id BINARY(16) NULL;
ALTER TABLE idempotency_keys ADD COLUMN claimed_at BIGINT NULL;
ALTER TABLE idempotency_keys ADD COLUMN completed_at BIGINT NULL;
