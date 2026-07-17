use std::collections::HashMap;

use runinator_models::value::{Map, Value};
use runinator_models::workflows::{
    WorkflowDefinition, WorkflowNode, WorkflowNodeKind, WorkflowStatus,
};

use crate::conditions::{evaluate_workflow_condition, next_transition};
use crate::errors::WorkflowValidationError;
use crate::expressions::{apply_input_defaults, evaluate_expression, resolve_value_refs};
use crate::parameters::{
    evaluate_percentage, evaluate_switch, evaluate_toggle, parse_output_parameters,
    parse_percentage_parameters, parse_switch_parameters, parse_toggle_parameters,
};

// upper bound on simulated steps; a runaway back-edge stops here instead of spinning forever.
const MAX_SIM_STEPS: usize = 10_000;

/// how a task or parked node resolved in a simulation. decouples the state-machine walk from any
/// concrete backend (a mock spec, or a database-backed replay).
#[derive(Debug, Clone, PartialEq)]
pub struct NodeOutcome {
    /// the terminal status the node reached.
    pub status: WorkflowStatus,
    /// the value recorded as the node's `output` (addressable downstream as `steps.<id>.output`).
    pub output: Value,
}

impl NodeOutcome {
    /// a succeeded outcome carrying `output`.
    pub fn succeeded(output: Value) -> Self {
        Self {
            status: WorkflowStatus::Succeeded,
            output,
        }
    }

    /// a failed outcome with a null output.
    pub fn failed() -> Self {
        Self {
            status: WorkflowStatus::Failed,
            output: Value::Null,
        }
    }
}

/// the request handed to the evaluator when the walk reaches a node whose outcome is not pure graph
/// logic — a task action, or a parked node awaiting an external decision.
pub struct NodeEvalRequest<'a> {
    /// the node being resolved.
    pub node: &'a WorkflowNode,
    /// the node's action configuration / parameters already resolved against the run context.
    pub resolved: Value,
    /// the full run context at this point in the walk (`input`, `steps`, `config`, ...).
    pub context: &'a Value,
}

/// the evaluator interface: supplies the parts of a workflow walk a pure graph simulation cannot
/// compute on its own — the `config.*` tree and the outcome of task/park nodes. Implementors back
/// this with a mock spec (offline tests) or a database (live replay); the walker stays identical.
pub trait SimulationEnv {
    /// the `config.*` reference tree merged into every node's context. Defaults to empty.
    fn config_tree(&mut self) -> Value {
        Value::Object(Map::new())
    }

    /// resolve a task (action) node: its simulated status and output.
    fn evaluate_action(&mut self, request: &NodeEvalRequest<'_>) -> NodeOutcome;

    /// resolve a parked node (approval/gate/signal/input/mutex/...). Defaults to succeeding with a
    /// null output so a park never blocks a simulation unless an env overrides it.
    fn resolve_park(&mut self, _request: &NodeEvalRequest<'_>) -> NodeOutcome {
        NodeOutcome::succeeded(Value::Null)
    }
}

/// one visited node in a simulation trace.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SimStep {
    pub node_id: String,
    pub kind: WorkflowNodeKind,
    pub status: WorkflowStatus,
    /// the next node the walk routed to, when the node had an outgoing edge.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
    /// the value recorded as this node's output, when it produced one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<Value>,
    /// a short reason string mirroring the reducer's transition reasons.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// the result of walking a workflow with a `SimulationEnv`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SimulationRun {
    /// the terminal status the run settled on.
    pub status: WorkflowStatus,
    /// the ordered nodes visited.
    pub steps: Vec<SimStep>,
    /// the run's final output (from the last output node, else null).
    pub output: Value,
    /// set when the walk could not continue: an unsupported node kind, a missing node, or a node
    /// that blocked with no outgoing edge.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl SimulationRun {
    /// true when a node with `node_id` was visited during the walk.
    pub fn reached(&self, node_id: &str) -> bool {
        self.steps.iter().any(|step| step.node_id == node_id)
    }

    /// the target the last visit to `node_id` routed to, if any. Used to assert which branch a
    /// condition/switch/toggle/percentage node took.
    pub fn branch_target(&self, node_id: &str) -> Option<&str> {
        self.steps
            .iter()
            .rev()
            .find(|step| step.node_id == node_id)
            .and_then(|step| step.next.as_deref())
    }

    /// the recorded output of the last visit to `node_id`, if any.
    pub fn node_output(&self, node_id: &str) -> Option<&Value> {
        self.steps
            .iter()
            .rev()
            .find(|step| step.node_id == node_id)
            .and_then(|step| step.output.as_ref())
    }
}

// the control decision a single node makes: continue to another node, or terminate the run.
enum Flow {
    Goto(String),
    Terminal,
}

/// walk a workflow's state machine from its start node, routing exactly as the reducer does for the
/// supported node kinds and deferring task/park outcomes to `env`. Loops/fan-out kinds
/// (loop/parallel/join/map/race/try/subflow) are not modelled and stop the walk with an `error`.
pub fn simulate_workflow(
    workflow: &WorkflowDefinition,
    inputs: Value,
    env: &mut dyn SimulationEnv,
) -> Result<SimulationRun, WorkflowValidationError> {
    let (start, nodes) = crate::validate_workflow(workflow)?;
    let node_by_id: HashMap<&str, &WorkflowNode> =
        nodes.iter().map(|node| (node.id.as_str(), node)).collect();

    let config = env.config_tree();
    let mut step_outputs: HashMap<String, Value> = HashMap::new();
    let mut last_output: Option<Value> = None;
    let mut steps: Vec<SimStep> = Vec::new();
    let mut run_output = Value::Null;
    let mut current = start;

    for _ in 0..MAX_SIM_STEPS {
        let Some(node) = node_by_id.get(current.as_str()).copied() else {
            return Ok(stuck(
                steps,
                run_output,
                format!("active node '{current}' is missing from the graph"),
            ));
        };

        // pre-output context, mirroring the reducer's `runtime_context`.
        let context = build_context(
            workflow,
            &inputs,
            &step_outputs,
            &config,
            last_output.clone(),
        );

        let Outcome {
            status,
            output,
            note,
            route_override,
        } = evaluate_node(node, &context, env)?;

        // publish this node's output before routing so its own branches can read `steps.<id>.output`.
        if let Some(output) = output.clone() {
            step_outputs.insert(node.id.clone(), output.clone());
            last_output = Some(output);
        }
        if node.kind == WorkflowNodeKind::Output {
            run_output = output.clone().unwrap_or(Value::Null);
        }

        // unsupported control-flow kinds cannot be simulated; stop with a clear diagnostic.
        if is_unsupported(&node.kind) {
            steps.push(SimStep {
                node_id: node.id.clone(),
                kind: node.kind.clone(),
                status,
                next: None,
                output,
                note: note.clone(),
            });
            return Ok(stuck(
                steps,
                run_output,
                format!(
                    "node '{}' of kind {} is not supported by the dry-run simulator",
                    node.id,
                    kind_label(&node.kind)
                ),
            ));
        }

        // terminal nodes settle the run.
        if matches!(node.kind, WorkflowNodeKind::End | WorkflowNodeKind::Fail) {
            steps.push(SimStep {
                node_id: node.id.clone(),
                kind: node.kind.clone(),
                status,
                next: None,
                output,
                note,
            });
            return Ok(SimulationRun {
                status,
                steps,
                output: run_output,
                error: None,
            });
        }

        // router nodes (switch/toggle/percentage) route to their evaluated target directly, exactly
        // as the reducer's `finish_route` does; everything else follows its transition edges.
        let flow = match route_override {
            Some(target) => Flow::Goto(target),
            None => {
                let routed = build_context(
                    workflow,
                    &inputs,
                    &step_outputs,
                    &config,
                    last_output.clone(),
                );
                route(node, status, &routed)?
            }
        };
        let next = match &flow {
            Flow::Goto(target) => Some(target.clone()),
            Flow::Terminal => None,
        };
        steps.push(SimStep {
            node_id: node.id.clone(),
            kind: node.kind.clone(),
            status,
            next: next.clone(),
            output,
            note,
        });
        match flow {
            Flow::Goto(target) => current = target,
            Flow::Terminal => {
                return Ok(SimulationRun {
                    status,
                    steps,
                    output: run_output,
                    error: None,
                });
            }
        }
    }

    Ok(stuck(
        steps,
        run_output,
        format!("simulation exceeded {MAX_SIM_STEPS} steps (possible loop)"),
    ))
}

// the snake_case wire label for a node kind (e.g. "parallel"), for diagnostics.
fn kind_label(kind: &WorkflowNodeKind) -> String {
    serde_json::to_value(kind)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| format!("{kind:?}"))
}

// kinds the simulator does not model; they need real fan-out/frame bookkeeping the walk lacks.
fn is_unsupported(kind: &WorkflowNodeKind) -> bool {
    matches!(
        kind,
        WorkflowNodeKind::Loop
            | WorkflowNodeKind::Parallel
            | WorkflowNodeKind::Join
            | WorkflowNodeKind::Map
            | WorkflowNodeKind::Race
            | WorkflowNodeKind::Try
            | WorkflowNodeKind::Subflow
    )
}

// a single node's computed result: its status, optional output, a short reason, and — for router
// nodes — the target the walk must jump to directly instead of following transition edges.
struct Outcome {
    status: WorkflowStatus,
    output: Option<Value>,
    note: Option<String>,
    route_override: Option<String>,
}

impl Outcome {
    fn new(status: WorkflowStatus, output: Option<Value>, note: &str) -> Self {
        Self {
            status,
            output,
            note: Some(note.to_string()),
            route_override: None,
        }
    }

    fn plain(status: WorkflowStatus) -> Self {
        Self {
            status,
            output: None,
            note: None,
            route_override: None,
        }
    }
}

// compute a single node's outcome, reusing the same evaluators as the reducer.
fn evaluate_node(
    node: &WorkflowNode,
    context: &Value,
    env: &mut dyn SimulationEnv,
) -> Result<Outcome, WorkflowValidationError> {
    let outcome = match node.kind {
        WorkflowNodeKind::Start => Outcome::new(WorkflowStatus::Succeeded, None, "start"),
        WorkflowNodeKind::End => Outcome::new(WorkflowStatus::Succeeded, None, "end_reached"),
        WorkflowNodeKind::Fail => Outcome::new(WorkflowStatus::Failed, None, "fail_reached"),
        WorkflowNodeKind::Wait => Outcome::plain(WorkflowStatus::Succeeded),
        WorkflowNodeKind::Audit | WorkflowNodeKind::Checkpoint => {
            Outcome::plain(WorkflowStatus::Succeeded)
        }
        WorkflowNodeKind::Config => {
            let resolved = resolve_value_refs(&node.parameters.clone().into(), context)
                .unwrap_or_else(|_| node.parameters.clone().into());
            Outcome::new(WorkflowStatus::Succeeded, Some(resolved), "config_applied")
        }
        WorkflowNodeKind::Condition => {
            let matched = evaluate_workflow_condition(&node.condition, context)?;
            let (status, reason) = if matched {
                (WorkflowStatus::Succeeded, "condition_matched")
            } else {
                (WorkflowStatus::Blocked, "condition_unmatched")
            };
            Outcome::new(status, None, reason)
        }
        WorkflowNodeKind::Switch => {
            let params = parse_switch_parameters(node)?;
            router_outcome(evaluate_switch(&params, context)?)
        }
        WorkflowNodeKind::Toggle => {
            let params = parse_toggle_parameters(node)?;
            router_outcome(Some(evaluate_toggle(&params, context)?))
        }
        WorkflowNodeKind::Percentage => {
            let params = parse_percentage_parameters(node)?;
            router_outcome(evaluate_percentage(&params, context)?)
        }
        WorkflowNodeKind::Transform => {
            let params: Value = node.parameters.clone().into();
            let bindings = params.get("bindings").cloned().unwrap_or(Value::Null);
            let resolved = resolve_value_refs(&bindings, context).unwrap_or(bindings);
            let output = runinator_models::json!({ "bindings": resolved });
            Outcome::new(WorkflowStatus::Succeeded, Some(output), "transform_applied")
        }
        WorkflowNodeKind::Assert => {
            let violations = evaluate_assertions(&node.parameters.clone().into(), context);
            let passed = violations.is_empty();
            let output = runinator_models::json!({
                "passed": passed,
                "violations": violations,
            });
            let (status, reason) = if passed {
                (WorkflowStatus::Succeeded, "assert_passed")
            } else {
                (WorkflowStatus::Failed, "assert_failed")
            };
            Outcome::new(status, Some(output), reason)
        }
        WorkflowNodeKind::Output => {
            let params = parse_output_parameters(node)?;
            let data = evaluate_expression(&params.data, context)?;
            Outcome::new(WorkflowStatus::Succeeded, Some(data), "output_emitted")
        }
        WorkflowNodeKind::Action => {
            let resolved = resolve_action_config(node, context);
            let outcome = env.evaluate_action(&NodeEvalRequest {
                node,
                resolved,
                context,
            });
            Outcome::new(outcome.status, Some(outcome.output), "action_result")
        }
        // every parked kind resolves through the evaluator's park hook.
        WorkflowNodeKind::Approval
        | WorkflowNodeKind::Gate
        | WorkflowNodeKind::Signal
        | WorkflowNodeKind::Input
        | WorkflowNodeKind::Mutex
        | WorkflowNodeKind::Throttle
        | WorkflowNodeKind::AwaitRun
        | WorkflowNodeKind::Debounce
        | WorkflowNodeKind::Collect
        | WorkflowNodeKind::Barrier
        | WorkflowNodeKind::CircuitBreaker
        | WorkflowNodeKind::EventSource => {
            let resolved: Value = node.parameters.clone().into();
            let outcome = env.resolve_park(&NodeEvalRequest {
                node,
                resolved,
                context,
            });
            let output = (!outcome.output.is_null()).then_some(outcome.output);
            Outcome::new(outcome.status, output, "park_resolved")
        }
        // unsupported kinds are flagged by the caller; give them a neutral outcome.
        WorkflowNodeKind::Loop
        | WorkflowNodeKind::Parallel
        | WorkflowNodeKind::Join
        | WorkflowNodeKind::Map
        | WorkflowNodeKind::Race
        | WorkflowNodeKind::Try
        | WorkflowNodeKind::Subflow => Outcome::plain(WorkflowStatus::Running),
    };
    Ok(outcome)
}

// a router node (switch/toggle/percentage): `Some` succeeds and jumps straight to the chosen target;
// `None` blocks (no bucket/case matched) and follows the node's failure edge.
fn router_outcome(target: Option<String>) -> Outcome {
    let output = runinator_models::json!({ "target": target });
    let status = if target.is_some() {
        WorkflowStatus::Succeeded
    } else {
        WorkflowStatus::Blocked
    };
    Outcome {
        status,
        output: Some(output),
        note: Some("route_evaluated".into()),
        route_override: target,
    }
}

// resolve an action node's configuration against context, tolerating unresolved refs by keeping the
// raw configuration (secrets stay unresolved in a simulation exactly as they would before a worker).
fn resolve_action_config(node: &WorkflowNode, context: &Value) -> Value {
    let raw: Value = node
        .action
        .as_ref()
        .map(|action| action.configuration.clone().into())
        .unwrap_or(Value::Null);
    resolve_value_refs(&raw, context).unwrap_or(raw)
}

// evaluate an assert node's assertions, mirroring the reducer's `evaluate_assertions`.
fn evaluate_assertions(params: &Value, context: &Value) -> Vec<Value> {
    let assertions = params
        .get("assertions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut violations = Vec::new();
    for assertion in &assertions {
        let name = assertion
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("unnamed")
            .to_string();
        let condition = assertion.get("condition").cloned().unwrap_or(Value::Null);
        let passed = crate::evaluate_condition(&condition, context).unwrap_or(false);
        if !passed {
            let message = assertion
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("Assertion failed")
                .to_string();
            violations.push(runinator_models::json!({ "name": name, "message": message }));
        }
    }
    violations
}

// pick the outgoing edge for a settled node, mirroring `next_transition`'s branch/next/failure rules.
fn route(
    node: &WorkflowNode,
    status: WorkflowStatus,
    context: &Value,
) -> Result<Flow, WorkflowValidationError> {
    match next_transition(node, status, context)? {
        Some(target) => Ok(Flow::Goto(target)),
        None => Ok(Flow::Terminal),
    }
}

// build the run context the same shape `runtime_context` produces: `input`, `steps`, `config`,
// `prev`, and a minimal `workflow` header, with omitted input fields filled from their defaults.
fn build_context(
    workflow: &WorkflowDefinition,
    inputs: &Value,
    step_outputs: &HashMap<String, Value>,
    config: &Value,
    prev: Option<Value>,
) -> Value {
    let mut context = crate::outputs_context(inputs, step_outputs);
    if let Some(object) = context.as_object_mut() {
        object.insert(
            "workflow".into(),
            runinator_models::json!({
                "run_id": Value::Null,
                "workflow_id": workflow.id,
                "state": Value::Object(Map::new()),
            }),
        );
        if let Some(prev) = prev {
            object.insert("prev".into(), prev);
        }
        object.insert("config".into(), config.clone());
    }
    apply_input_defaults(&mut context, &workflow.input_type);
    context
}

fn stuck(steps: Vec<SimStep>, output: Value, error: String) -> SimulationRun {
    SimulationRun {
        status: WorkflowStatus::Blocked,
        steps,
        output,
        error: Some(error),
    }
}

#[cfg(test)]
mod tests;
