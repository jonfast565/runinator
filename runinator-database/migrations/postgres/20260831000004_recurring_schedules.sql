ALTER TABLE freeze_windows ADD COLUMN schedule TEXT NULL;
ALTER TABLE pipeline_trigger_firings ADD COLUMN outcome TEXT NOT NULL DEFAULT 'fired';
