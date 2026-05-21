use runinator_models::workflows::WorkflowBundle;

#[test]
fn bundled_seed_file_contains_no_default_workflows_or_triggers() {
    let parsed: WorkflowBundle = serde_json::from_str(include_str!("../workflows/workflows.json"))
        .expect("seed file parses");

    assert!(parsed.workflows.is_empty());
    assert!(parsed.triggers.is_empty());
}
