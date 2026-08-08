-- the cooldown gate's window, as columns rather than a json blob.
--
-- it lived in `automation_records.data`, which made the gate impossible to claim atomically: the
-- reducer read the record, decided, then stamped it, so two runs hitting one gate concurrently both
-- saw an elapsed window and both entered the body. that is the single condition the gate exists to
-- prevent. predicating on `last_run_at` inside a json blob is what the three dialects disagree
-- about most, so the fix is a real column and a conditional UPDATE whose affected-row count decides
-- the winner.
--
-- windows in flight at upgrade time are not carried over: a gate opens once more than it strictly
-- should, then behaves. that is the cheap direction to be wrong in.
CREATE TABLE IF NOT EXISTS workflow_cooldowns (
    name TEXT PRIMARY KEY,
    last_run_at BIGINT NOT NULL
);
