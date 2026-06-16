-- add the workflow identity namespace (from a `namespace <path>` header). nullable so
-- existing unqualified workflows are unaffected; a subflow target "<namespace>.<name>" resolves
-- against the qualified identity namespace || '.' || name.
ALTER TABLE workflows ADD COLUMN namespace TEXT;
