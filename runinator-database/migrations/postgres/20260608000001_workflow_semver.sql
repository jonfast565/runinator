-- store the workflow version as a semantic version string (major.minor.patch).
-- existing integer versions migrate to "<n>.0.0".
ALTER TABLE workflows
    ALTER COLUMN version TYPE TEXT USING (version::text || '.0.0');
