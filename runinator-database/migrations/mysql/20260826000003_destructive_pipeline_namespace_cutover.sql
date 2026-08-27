-- Pipeline members and chained trigger sources now use canonical namespace paths and resolved
-- UUIDs. Legacy pipeline graphs cannot be reconstructed safely from their display-name edges, so
-- this intentionally removes every pipeline and its live execution history. Reapply source packs
-- after this migration to create new, namespaced pipeline definitions.

DELETE FROM workflow_triggers
WHERE JSON_EXTRACT(configuration, '$.pipeline_id') IS NOT NULL;

DELETE FROM pipeline_trigger_firings
WHERE pipeline_run_id IN (SELECT id FROM pipeline_runs);
DELETE FROM pipeline_trigger_firings
WHERE trigger_id IN (SELECT id FROM pipeline_triggers);
DELETE FROM pipeline_triggers;

DELETE FROM pipeline_member_attempts
WHERE pipeline_run_id IN (SELECT id FROM pipeline_runs);

DELETE FROM workflow_effect_output_events
WHERE workflow_run_id IN (SELECT id FROM workflow_runs WHERE pipeline_run_id IS NOT NULL);
DELETE FROM workflow_effect_dispatches
WHERE effect_id IN (
    SELECT id FROM workflow_effects
    WHERE workflow_run_id IN (SELECT id FROM workflow_runs WHERE pipeline_run_id IS NOT NULL)
);
DELETE FROM workflow_journal_entries
WHERE workflow_run_id IN (SELECT id FROM workflow_runs WHERE pipeline_run_id IS NOT NULL);
DELETE FROM workflow_effects
WHERE workflow_run_id IN (SELECT id FROM workflow_runs WHERE pipeline_run_id IS NOT NULL);
DELETE FROM workflow_continuations
WHERE workflow_run_id IN (SELECT id FROM workflow_runs WHERE pipeline_run_id IS NOT NULL);
DELETE FROM workflow_vm_modules
WHERE workflow_run_id IN (SELECT id FROM workflow_runs WHERE pipeline_run_id IS NOT NULL);
DELETE FROM workflow_trigger_firings
WHERE workflow_run_id IN (SELECT id FROM workflow_runs WHERE pipeline_run_id IS NOT NULL);
DELETE FROM workflow_runs WHERE pipeline_run_id IS NOT NULL;

DELETE FROM pipeline_runs;
DELETE FROM pipeline_revisions;
DELETE FROM resource_grants WHERE resource_type = 'pipeline';
DELETE FROM resource_ownership WHERE resource_type = 'pipeline';
DELETE FROM pipelines;
