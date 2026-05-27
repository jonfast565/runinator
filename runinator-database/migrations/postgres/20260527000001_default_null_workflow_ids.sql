CREATE OR REPLACE FUNCTION runinator_default_workflow_id()
RETURNS trigger AS $$
BEGIN
    IF NEW.id IS NULL THEN
        NEW.id := nextval('workflows_id_seq');
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS default_workflow_id ON workflows;
CREATE TRIGGER default_workflow_id
BEFORE INSERT ON workflows
FOR EACH ROW
EXECUTE FUNCTION runinator_default_workflow_id();

CREATE OR REPLACE FUNCTION runinator_default_workflow_trigger_id()
RETURNS trigger AS $$
BEGIN
    IF NEW.id IS NULL THEN
        NEW.id := nextval('workflow_triggers_id_seq');
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS default_workflow_trigger_id ON workflow_triggers;
CREATE TRIGGER default_workflow_trigger_id
BEFORE INSERT ON workflow_triggers
FOR EACH ROW
EXECUTE FUNCTION runinator_default_workflow_trigger_id();
