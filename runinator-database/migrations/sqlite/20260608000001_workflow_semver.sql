-- store the workflow version as a semantic version string (major.minor.patch).
-- the column keeps its declared affinity, but the dotted string is not a well-formed
-- integer so sqlite stores it as text; existing integer versions migrate to "<n>.0.0".
UPDATE workflows SET version = CAST(version AS TEXT) || '.0.0';
