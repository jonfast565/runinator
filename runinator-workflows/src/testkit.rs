use std::collections::HashMap;

use runinator_models::value::Value;
use runinator_models::workflows::{WorkflowDefinition, WorkflowStatus};
use serde::Deserialize;

use crate::simulate::{
    NodeEvalRequest, NodeOutcome, SimStep, SimulationEnv, SimulationRun, simulate_workflow,
};

/// the test implementation of `SimulationEnv`: config and task/park outcomes come from a fixed spec,
/// with a default success outcome for any node the spec does not name.

fn default_mock_status() -> WorkflowStatus {
    WorkflowStatus::Succeeded
}

impl From<&MockSpec> for NodeOutcome {
    fn from(spec: &MockSpec) -> Self {
        NodeOutcome {
            status: spec.status,
            output: spec.output.clone(),
        }
    }
}

/// run one case against a compiled workflow: simulate with a `MockEnv`, then check expectations.
pub fn run_test_case(workflow: &WorkflowDefinition, case: &WorkflowTestCase) -> TestCaseResult {
    let outcomes = case
        .mocks
        .iter()
        .map(|(id, spec)| (id.clone(), NodeOutcome::from(spec)))
        .collect();
    let mut env = MockEnv::new(case.config.clone(), outcomes);

    let run = match simulate_workflow(workflow, case.input.clone(), &mut env) {
        Ok(run) => run,
        Err(error) => {
            return TestCaseResult {
                name: case.name.clone(),
                passed: false,
                failures: vec![format!("simulation could not run: {error}")],
                run: SimulationRun {
                    status: WorkflowStatus::Failed,
                    steps: Vec::<SimStep>::new(),
                    output: Value::Null,
                    error: Some(error.to_string()),
                },
            };
        }
    };

    let failures = check_expectations(&case.expect, &run);
    TestCaseResult {
        name: case.name.clone(),
        passed: failures.is_empty(),
        failures,
        run,
    }
}

fn check_expectations(expect: &Expectations, run: &SimulationRun) -> Vec<String> {
    let mut failures = Vec::new();

    if let Some(expected) = expect.error {
        let actual = run.error.is_some();
        if actual != expected {
            match &run.error {
                Some(message) if !expected => {
                    failures.push(format!(
                        "expected no error, but the walk stopped: {message}"
                    ));
                }
                _ if expected => {
                    failures.push("expected the walk to get stuck, but it completed".into())
                }
                _ => {}
            }
        }
    }

    if let Some(expected) = expect.status
        && run.status != expected
    {
        failures.push(format!(
            "expected status {}, got {}",
            expected.as_str(),
            run.status.as_str()
        ));
    }

    for node in &expect.reached {
        if !run.reached(node) {
            failures.push(format!(
                "expected node '{node}' to be reached, but it was not"
            ));
        }
    }
    for node in &expect.not_reached {
        if run.reached(node) {
            failures.push(format!(
                "expected node '{node}' not to be reached, but it was"
            ));
        }
    }

    for (node, expected_target) in &expect.branches {
        match run.branch_target(node) {
            Some(target) if target == expected_target => {}
            Some(target) => failures.push(format!(
                "expected node '{node}' to route to '{expected_target}', but it routed to '{target}'"
            )),
            None => failures.push(format!(
                "expected node '{node}' to route to '{expected_target}', but it was not reached or had no edge"
            )),
        }
    }

    if let Some(expected) = &expect.output
        && &run.output != expected
    {
        failures.push(format!("expected output {expected}, got {}", run.output));
    }

    if let Some(subset) = &expect.output_contains
        && let Some(message) = output_subset_mismatch(subset, &run.output)
    {
        failures.push(message);
    }

    failures
}

// recursive subset check: every key in `subset` (objects) or index (arrays) must match `actual`.
fn output_subset_mismatch(subset: &Value, actual: &Value) -> Option<String> {
    match (subset, actual) {
        (Value::Object(expected), Value::Object(got)) => {
            for (key, value) in expected {
                match got.get(key) {
                    Some(inner) => {
                        if let Some(message) = output_subset_mismatch(value, inner) {
                            return Some(message);
                        }
                    }
                    None => return Some(format!("expected output to contain key '{key}'")),
                }
            }
            None
        }
        (expected, got) if expected == got => None,
        (expected, got) => Some(format!("expected output to contain {expected}, got {got}")),
    }
}

#[cfg(test)]
mod tests;

mod mock_env;
pub use mock_env::MockEnv;

mod workflow_test_suite;
pub use workflow_test_suite::WorkflowTestSuite;

mod workflow_test_case;
pub use workflow_test_case::WorkflowTestCase;

mod mock_spec;
pub use mock_spec::MockSpec;

mod expectations;
pub use expectations::Expectations;

mod test_case_result;
pub use test_case_result::TestCaseResult;
