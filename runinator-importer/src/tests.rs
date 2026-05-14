use super::ImportFile;

#[test]
fn bundled_seed_file_contains_valid_workflows_and_triggers() {
    let parsed: ImportFile =
        serde_json::from_str(include_str!("../tasks/tasks.json")).expect("seed file parses");

    assert!(!parsed.workflows.is_empty());
    assert!(!parsed.triggers.is_empty());

    for workflow in parsed.workflows {
        let workflow = runinator_workflows::normalize_workflow(&workflow);
        runinator_workflows::validate_workflow(&workflow).expect("workflow is valid");
    }
}
