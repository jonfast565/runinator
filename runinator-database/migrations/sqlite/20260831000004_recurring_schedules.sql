-- Portable recurrence definition; starts_at/ends_at remain the materialized active-or-next window
-- used by the trigger claim hot path.
ALTER TABLE freeze_windows ADD COLUMN schedule TEXT NULL;
ALTER TABLE pipeline_trigger_firings ADD COLUMN outcome TEXT NOT NULL DEFAULT 'fired';
