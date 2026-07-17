use std::collections::HashMap;

use runinator_models::value::Value;
use runinator_models::workflows::{WorkflowDefinition, WorkflowStatus};
use serde::Deserialize;

use crate::simulate::{
    NodeEvalRequest, NodeOutcome, SimStep, SimulationEnv, SimulationRun, simulate_workflow,
};

/// the test implementation of `SimulationEnv`: config and task/park outcomes come from a fixed spec,
/// with a default success outcome for any node the spec does not name.
pub struct MockEnv {
    config: Value,
    outcomes: HashMap<String, NodeOutcome>,
    default_outcome: NodeOutcome,
}

impl MockEnv {
    /// build a mock env from a `config.*` tree and per-node outcomes keyed by node id.
    pub fn new(config: Value, outcomes: HashMap<String, NodeOutcome>) -> Self {
        Self {
            config,
            outcomes,
            default_outcome: NodeOutcome::succeeded(Value::Null),
        }
    }

    /// override the outcome used for a node the spec does not explicitly mock.
    pub fn with_default(mut self, outcome: NodeOutcome) -> Self {
        self.default_outcome = outcome;
        self
    }

    fn outcome_for(&self, node_id: &str) -> NodeOutcome {
        self.outcomes
            .get(node_id)
            .cloned()
            .unwrap_or_else(|| self.default_outcome.clone())
    }
}

impl SimulationEnv for MockEnv {
    fn config_tree(&mut self) -> Value {
        self.config.clone()
    }

    fn evaluate_action(&mut self, request: &NodeEvalRequest<'_>) -> NodeOutcome {
        self.outcome_for(&request.node.id)
    }

    fn resolve_park(&mut self, request: &NodeEvalRequest<'_>) -> NodeOutcome {
        self.outcome_for(&request.node.id)
    }
}

/// a `.wdlt` test suite: a set of cases run against one compiled workflow (or, for multi-workflow
/// packs, the workflow each case names).
#[derive(Debug, Clone, Deserialize)]
pub struct WorkflowTestSuite {
    /// default workflow name for cases that do not name their own; optional for single-workflow packs.
    #[serde(default)]
    pub workflow: Option<String>,
    pub tests: Vec<WorkflowTestCase>,
}

/// one case: inputs, config fixtures, mocked node outcomes, and expectations to assert.
#[derive(Debug, Clone, Deserialize)]
pub struct WorkflowTestCase {
    pub name: String,
    /// the workflow this case targets, overriding the suite default.
    #[serde(default)]
    pub workflow: Option<String>,
    #[serde(default)]
    pub input: Value,
    /// the `config.*` tree exposed to expressions, shaped `{ scope: { name: value } }`.
    #[serde(default)]
    pub config: Value,
    /// mocked task/park outcomes keyed by node id.
    #[serde(default)]
    pub mocks: HashMap<String, MockSpec>,
    #[serde(default)]
    pub expect: Expectations,
}

/// a mocked node outcome in a test spec.
#[derive(Debug, Clone, Deserialize)]
pub struct MockSpec {
    #[serde(default)]
    pub output: Value,
    #[serde(default = "default_mock_status")]
    pub status: WorkflowStatus,
}

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

/// what a case asserts about the resulting simulation.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Expectations {
    /// the terminal run status.
    #[serde(default)]
    pub status: Option<WorkflowStatus>,
    /// nodes that must be visited.
    #[serde(default)]
    pub reached: Vec<String>,
    /// nodes that must not be visited.
    #[serde(default)]
    pub not_reached: Vec<String>,
    /// per-router-node expected next target: `{ "gate": "on" }`.
    #[serde(default)]
    pub branches: HashMap<String, String>,
    /// exact match on the run's final output.
    #[serde(default)]
    pub output: Option<Value>,
    /// subset match: every key/value here must be present in the final output object.
    #[serde(default)]
    pub output_contains: Option<Value>,
    /// whether the walk is expected to get stuck (an unsupported/blocked node with no edge).
    #[serde(default)]
    pub error: Option<bool>,
}

/// the outcome of running one case.
#[derive(Debug, Clone)]
pub struct TestCaseResult {
    pub name: String,
    pub passed: bool,
    /// human-readable assertion failures; empty when the case passed.
    pub failures: Vec<String>,
    pub run: SimulationRun,
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

    if let Some(expected) = expect.status {
        if run.status != expected {
            failures.push(format!(
                "expected status {}, got {}",
                expected.as_str(),
                run.status.as_str()
            ));
        }
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

    if let Some(expected) = &expect.output {
        if &run.output != expected {
            failures.push(format!("expected output {expected}, got {}", run.output));
        }
    }

    if let Some(subset) = &expect.output_contains {
        if let Some(message) = output_subset_mismatch(subset, &run.output) {
            failures.push(message);
        }
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
