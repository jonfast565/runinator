use runinator_models::workflows::WorkflowDefinition;

use crate::testkit::{WorkflowTestSuite, run_test_case};

fn workflow() -> WorkflowDefinition {
    serde_json::from_value(serde_json::json!({
        "name": "approval-flow",
        "definition": {
            "start": "start",
            "nodes": [
                { "id": "start", "kind": "start", "transitions": { "next": { "$node": "amount" } } },
                {
                    "id": "amount",
                    "kind": "condition",
                    "condition": {
                        "value": { "$ref": { "input": ["amount"] } },
                        "greater_than": 100
                    },
                    "transitions": {
                        "on_success": { "$node": "review" },
                        "on_failure": { "$node": "auto" }
                    }
                },
                {
                    "id": "review",
                    "kind": "action",
                    "action": { "provider": "console", "function": "log", "configuration": {} },
                    "transitions": { "next": { "$node": "out" } }
                },
                { "id": "auto", "kind": "end" },
                {
                    "id": "out",
                    "kind": "output",
                    "parameters": { "data": { "$ref": { "node": "review", "output": [] } } },
                    "transitions": { "next": { "$node": "done" } }
                },
                { "id": "done", "kind": "end" }
            ]
        }
    }))
    .expect("workflow")
}

#[test]
fn suite_parses_and_runs_cases() {
    let suite: WorkflowTestSuite = serde_json::from_value(serde_json::json!({
        "tests": [
            {
                "name": "small amount auto-approves",
                "input": { "amount": 10 },
                "expect": { "status": "succeeded", "reached": ["auto"], "not_reached": ["review"] }
            },
            {
                "name": "large amount routes to review",
                "input": { "amount": 500 },
                "mocks": { "review": { "output": { "approved": true } } },
                "expect": {
                    "status": "succeeded",
                    "reached": ["review", "out"],
                    "not_reached": ["auto"],
                    "output_contains": { "approved": true }
                }
            }
        ]
    }))
    .expect("suite");

    let def = workflow();
    for case in &suite.tests {
        let result = run_test_case(&def, case);
        assert!(
            result.passed,
            "case '{}' failed: {:?}",
            result.name, result.failures
        );
    }
}

#[test]
fn failed_expectation_reports_a_readable_failure() {
    let suite: WorkflowTestSuite = serde_json::from_value(serde_json::json!({
        "tests": [
            {
                "name": "wrong branch expectation",
                "input": { "amount": 10 },
                "expect": { "reached": ["review"] }
            }
        ]
    }))
    .expect("suite");

    let def = workflow();
    let result = run_test_case(&def, &suite.tests[0]);
    assert!(!result.passed);
    assert!(result.failures.iter().any(|f| f.contains("review")));
}
