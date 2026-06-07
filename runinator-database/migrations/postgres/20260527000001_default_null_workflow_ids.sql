-- Obsolete under UUID keys: ids are generated app-side, so the old NULL-id backfill triggers that
-- pulled from the bigserial sequences are no longer needed. Drop them if a prior migration created
-- them; on a fresh UUID schema these are no-ops.
DROP TRIGGER IF EXISTS default_workflow_id ON workflows;
DROP FUNCTION IF EXISTS runinator_default_workflow_id();
DROP TRIGGER IF EXISTS default_workflow_trigger_id ON workflow_triggers;
DROP FUNCTION IF EXISTS runinator_default_workflow_trigger_id();
