use super::ImportFile;

#[test]
fn bundled_seed_file_contains_valid_workflows() {
    let parsed: ImportFile =
        serde_json::from_str(include_str!("../tasks/tasks.json")).expect("seed file parses");

    assert!(!parsed.tasks.is_empty());
    assert!(!parsed.workflows.is_empty());

    for workflow in parsed.workflows {
        runinator_workflows::validate_workflow(&workflow).expect("workflow is valid");
    }
}
