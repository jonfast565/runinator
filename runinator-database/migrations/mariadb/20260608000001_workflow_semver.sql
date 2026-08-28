-- store the workflow version as a semantic version string (major.minor.patch).
-- widen the column first, then migrate existing integer versions to "<n>.0.0".
ALTER TABLE workflows MODIFY version TEXT NOT NULL;
UPDATE workflows SET version = CONCAT(version, '.0.0');
