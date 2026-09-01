ALTER TABLE freeze_windows ADD COLUMN schedule LONGTEXT NULL;
ALTER TABLE pipeline_trigger_firings ADD COLUMN outcome VARCHAR(32) NOT NULL DEFAULT 'fired';
