-- optimistic-concurrency guard for the run state blob. every state write is a read-modify-write
-- of the whole object, so two drives of the same run (one per cursor, once a run holds several)
-- would otherwise silently discard each other's frames. writers compare-and-swap on this and
-- retry when it moves under them.
ALTER TABLE workflow_runs ADD COLUMN state_version BIGINT NOT NULL DEFAULT 0;
