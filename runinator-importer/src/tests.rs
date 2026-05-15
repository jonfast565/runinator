use super::ImportFile;
use std::collections::HashSet;

#[test]
fn bundled_seed_file_contains_valid_workflows_and_triggers() {
    let parsed: ImportFile = serde_json::from_str(include_str!("../workflows/workflows.json"))
        .expect("seed file parses");

    assert!(!parsed.workflows.is_empty());
    assert!(!parsed.triggers.is_empty());

    let workflow_ids = parsed
        .workflows
        .iter()
        .map(|workflow| workflow.id.expect("seed workflows have stable ids"))
        .collect::<HashSet<_>>();

    assert!(parsed.workflows.iter().all(|workflow| workflow.enabled));
    assert!(parsed.triggers.iter().all(|trigger| trigger.enabled));
    assert!(
        parsed
            .triggers
            .iter()
            .all(|trigger| workflow_ids.contains(&trigger.workflow_id))
    );

    for workflow in parsed.workflows {
        let workflow = runinator_workflows::normalize_workflow(&workflow);
        let (_start, nodes) =
            runinator_workflows::validate_workflow(&workflow).expect("workflow is valid");

        for node in nodes.into_iter().filter(|node| node.action.is_some()) {
            let serialized = serde_json::to_value(&node).expect("action node serializes");
            let action = serialized
                .get("action")
                .and_then(serde_json::Value::as_object)
                .expect("action node serializes action as object");
            assert!(action.get("provider").is_some());
            assert!(action.get("function").is_some());
            assert!(action.get("action_name").is_none());
            assert!(action.get("action_function").is_none());
        }
    }
}
