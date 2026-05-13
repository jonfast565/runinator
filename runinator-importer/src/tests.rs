use super::{ImportFile, mark_workflow_tasks, workflow_task_ids};
use runinator_models::{core::ScheduledTask, workflows::WorkflowDefinition};
use serde_json::{Value, json};

#[test]
fn bundled_seed_file_contains_valid_workflows() {
    let parsed: ImportFile =
        serde_json::from_str(include_str!("../tasks/tasks.json")).expect("seed file parses");

    assert!(!parsed.tasks.is_empty());
    assert!(!parsed.workflows.is_empty());

    for workflow in parsed.workflows {
        let workflow = runinator_workflows::normalize_workflow(&workflow);
        runinator_workflows::validate_workflow(&workflow).expect("workflow is valid");
    }
}

#[test]
fn workflow_referenced_tasks_are_marked_as_workflow_tasks() {
    let workflows = vec![WorkflowDefinition {
        id: Some(1),
        name: "workflow".into(),
        version: 1,
        enabled: true,
        input_schema: json!({}),
        definition: json!({
            "nodes": [
                { "id": "task", "kind": "task", "task_id": 42 },
                { "id": "done", "kind": "end" }
            ]
        }),
        created_at: None,
        updated_at: None,
    }];
    let workflow_task_ids = workflow_task_ids(&workflows);
    let mut tasks = vec![task(42), task(43)];

    mark_workflow_tasks(&mut tasks, &workflow_task_ids);

    assert_eq!(
        tasks[0].metadata.get("task_type").and_then(Value::as_str),
        Some("workflow")
    );
    assert!(tasks[1].metadata.get("task_type").is_none());
}

fn task(id: i64) -> ScheduledTask {
    ScheduledTask {
        id: Some(id),
        name: format!("task {id}"),
        cron_schedule: "* * * * *".into(),
        action_name: "provider".into(),
        action_function: "run".into(),
        timeout: 30,
        next_execution: None,
        enabled: false,
        immediate: false,
        blackout_start: None,
        blackout_end: None,
        default_parameters: json!({}),
        mcp_enabled: false,
        metadata: json!({}),
        tags: vec![],
    }
}
