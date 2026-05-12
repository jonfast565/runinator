use std::collections::{HashMap, HashSet};

use runinator_models::workflows::{
    WorkflowDefinition, WorkflowNode, WorkflowNodeKind, WorkflowStatus, WorkflowTransitions,
};
use serde_json::{Map, Value};
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum WorkflowValidationError {
    #[error("workflow definition.nodes must be an array")]
    MissingNodes,
    #[error("workflow definition.start must name the first node")]
    MissingStart,
    #[error("workflow node '{0}' is duplicated")]
    DuplicateNode(String),
    #[error("workflow start node '{0}' does not exist")]
    MissingStartNode(String),
    #[error("workflow definition.start must reference a start node")]
    MissingStartKind,
    #[error("workflow must include an end node")]
    MissingEndNode,
    #[error("workflow node '{node}' references missing node '{target}'")]
    MissingTransition { node: String, target: String },
    #[error("workflow node is invalid: {0}")]
    InvalidNode(String),
    #[error("workflow node '{0}' of kind task requires task_id")]
    MissingTaskId(String),
    #[error("workflow node '{0}' retry.max_attempts must be greater than zero")]
    InvalidRetry(String),
    #[error("workflow node '{0}' timeout_seconds must be greater than zero")]
    InvalidTimeout(String),
    #[error("workflow node '{0}' max_iterations must be greater than zero")]
    InvalidLoopLimit(String),
    #[error("workflow node '{0}' uses unsupported local $ref cycle")]
    RefCycle(String),
    #[error("workflow $ref '{0}' could not be resolved")]
    MissingRef(String),
    #[error("runtime value reference '{0}' is invalid")]
    InvalidValueRef(String),
    #[error("declarative condition is invalid: {0}")]
    InvalidCondition(String),
    #[error("workflow node '{node}' parameters are invalid: {message}")]
    InvalidNodeParameters { node: String, message: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchPolicy {
    All,
    Any,
    FirstSuccess,
}

impl BranchPolicy {
    pub fn parse(value: Option<&Value>, default: BranchPolicy) -> Result<Self, String> {
        match value.and_then(Value::as_str) {
            None => Ok(default),
            Some("all") => Ok(BranchPolicy::All),
            Some("any") => Ok(BranchPolicy::Any),
            Some("first_success") => Ok(BranchPolicy::FirstSuccess),
            Some(other) => Err(format!("unsupported branch policy '{other}'")),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SwitchCase {
    pub target: String,
    pub condition: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SwitchParameters {
    pub value: Value,
    pub cases: Vec<SwitchCase>,
    pub default: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParallelParameters {
    pub branches: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct JoinParameters {
    pub wait_for: Vec<String>,
    pub mode: BranchPolicy,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TryParameters {
    pub body: String,
    pub catch: Option<String>,
    pub finally: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MapParameters {
    pub items: Value,
    pub target: String,
    pub concurrency: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RaceParameters {
    pub branches: Vec<String>,
    pub winner: BranchPolicy,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EmitParameters {
    pub event_type: Option<String>,
    pub data: Value,
}

pub fn normalize_workflow(workflow: &WorkflowDefinition) -> WorkflowDefinition {
    let mut normalized = workflow.clone();
    normalized.definition = normalize_definition(workflow.definition.clone());
    normalized
}

pub fn normalize_definition(definition: Value) -> Value {
    let mut root = match definition {
        Value::Object(root) => root,
        _ => Map::new(),
    };
    normalize_layout(&mut root);

    let mut nodes = match root.remove("nodes") {
        Some(Value::Array(nodes)) => nodes,
        _ => Vec::new(),
    };
    let mut ids = node_ids(&nodes);
    let existing_start = root
        .get("start")
        .and_then(Value::as_str)
        .map(str::to_string);
    let end_id = ensure_end_node(&mut nodes, &mut ids);
    let previous_start = existing_start
        .filter(|id| ids.contains(id) && node_kind_by_id(&nodes, id).as_deref() != Some("start"))
        .or_else(|| first_node_id(&nodes, |kind| kind != Some("start") && kind != Some("end")))
        .unwrap_or_else(|| end_id.clone());
    let start_id = ensure_start_node(&mut nodes, &mut ids, &previous_start, &end_id);

    route_success_terminals_to_end(&mut nodes, &end_id);
    root.insert("start".into(), Value::String(start_id));
    root.insert("nodes".into(), Value::Array(nodes));
    Value::Object(root)
}

pub fn expand_workflow_refs(
    workflow: &WorkflowDefinition,
) -> Result<Value, WorkflowValidationError> {
    let mut root = workflow.definition.clone();
    let defs = root
        .get("$defs")
        .cloned()
        .unwrap_or(Value::Object(Map::new()));
    let mut stack = Vec::new();
    expand_refs_in_value(&mut root, &defs, &mut stack)?;
    Ok(root)
}

pub fn parse_nodes(
    workflow: &WorkflowDefinition,
) -> Result<(String, Vec<WorkflowNode>), WorkflowValidationError> {
    let definition = expand_workflow_refs(workflow)?;
    let start = definition
        .get("start")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or(WorkflowValidationError::MissingStart)?
        .to_string();
    let nodes = definition
        .get("nodes")
        .and_then(Value::as_array)
        .ok_or(WorkflowValidationError::MissingNodes)?;
    let nodes = nodes
        .iter()
        .map(|value| {
            serde_json::from_value(value.clone())
                .map_err(|err| WorkflowValidationError::InvalidNode(err.to_string()))
        })
        .collect::<Result<Vec<WorkflowNode>, _>>()?;
    Ok((start, nodes))
}

pub fn validate_workflow(
    workflow: &WorkflowDefinition,
) -> Result<(String, Vec<WorkflowNode>), WorkflowValidationError> {
    let (start, nodes) = parse_nodes(workflow)?;
    let mut seen = HashSet::new();
    let ids = nodes
        .iter()
        .map(|node| {
            if !seen.insert(node.id.clone()) {
                return Err(WorkflowValidationError::DuplicateNode(node.id.clone()));
            }
            Ok(node.id.clone())
        })
        .collect::<Result<HashSet<_>, _>>()?;

    if !ids.contains(&start) {
        return Err(WorkflowValidationError::MissingStartNode(start));
    }
    if nodes
        .iter()
        .find(|node| node.id == start)
        .is_none_or(|node| node.kind != WorkflowNodeKind::Start)
    {
        return Err(WorkflowValidationError::MissingStartKind);
    }
    if !nodes.iter().any(|node| node.kind == WorkflowNodeKind::End) {
        return Err(WorkflowValidationError::MissingEndNode);
    }

    for node in &nodes {
        if node.kind == WorkflowNodeKind::Task && node.task_id.is_none() {
            return Err(WorkflowValidationError::MissingTaskId(node.id.clone()));
        }
        if node.retry.max_attempts <= 0 {
            return Err(WorkflowValidationError::InvalidRetry(node.id.clone()));
        }
        if node.timeout_seconds.is_some_and(|timeout| timeout <= 0) {
            return Err(WorkflowValidationError::InvalidTimeout(node.id.clone()));
        }
        if node.max_iterations.is_some_and(|limit| limit <= 0) {
            return Err(WorkflowValidationError::InvalidLoopLimit(node.id.clone()));
        }
        validate_condition(&node.condition)?;
        validate_control_node_parameters(node)?;
        for target in transition_targets(&node.transitions) {
            if !ids.contains(&target) {
                return Err(WorkflowValidationError::MissingTransition {
                    node: node.id.clone(),
                    target,
                });
            }
        }
        for target in parameter_targets(node)? {
            if !ids.contains(&target) {
                return Err(WorkflowValidationError::MissingTransition {
                    node: node.id.clone(),
                    target,
                });
            }
        }
    }

    validate_graph_cycles(&start, &nodes)?;

    Ok((start, nodes))
}

pub fn parse_switch_parameters(
    node: &WorkflowNode,
) -> Result<SwitchParameters, WorkflowValidationError> {
    let object = parameter_object(node)?;
    let value = object
        .get("value")
        .cloned()
        .ok_or_else(|| invalid_parameters(node, "switch.value is required"))?;
    let cases = object
        .get("cases")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_parameters(node, "switch.cases must be an array"))?;
    let cases = cases
        .iter()
        .map(|case| {
            let case_object = case
                .as_object()
                .ok_or_else(|| invalid_parameters(node, "switch case must be an object"))?;
            let target = case_object
                .get("target")
                .and_then(Value::as_str)
                .filter(|target| !target.is_empty())
                .ok_or_else(|| invalid_parameters(node, "switch case target is required"))?
                .to_string();
            let condition = if let Some(when) = case_object.get("when") {
                when.clone()
            } else {
                let mut condition = Map::new();
                condition.insert("value".into(), value.clone());
                for key in ["equals", "not_equals", "exists"] {
                    if let Some(expected) = case_object.get(key) {
                        condition.insert(key.into(), expected.clone());
                    }
                }
                if condition.len() == 1 {
                    return Err(invalid_parameters(
                        node,
                        "switch case requires when, equals, not_equals, or exists",
                    ));
                }
                Value::Object(condition)
            };
            Ok(SwitchCase { target, condition })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let default = object
        .get("default")
        .and_then(Value::as_str)
        .filter(|target| !target.is_empty())
        .map(str::to_string);
    Ok(SwitchParameters {
        value,
        cases,
        default,
    })
}

pub fn parse_parallel_parameters(
    node: &WorkflowNode,
) -> Result<ParallelParameters, WorkflowValidationError> {
    let object = parameter_object(node)?;
    let branches = string_array(object.get("branches"), node, "parallel.branches")?;
    if branches.is_empty() {
        return Err(invalid_parameters(
            node,
            "parallel.branches cannot be empty",
        ));
    }
    Ok(ParallelParameters { branches })
}

pub fn parse_join_parameters(
    node: &WorkflowNode,
) -> Result<JoinParameters, WorkflowValidationError> {
    let object = parameter_object(node)?;
    let wait_for = string_array(object.get("wait_for"), node, "join.wait_for")?;
    if wait_for.is_empty() {
        return Err(invalid_parameters(node, "join.wait_for cannot be empty"));
    }
    let mode = BranchPolicy::parse(object.get("mode"), BranchPolicy::All)
        .map_err(|message| invalid_parameters(node, message))?;
    Ok(JoinParameters { wait_for, mode })
}

pub fn parse_try_parameters(node: &WorkflowNode) -> Result<TryParameters, WorkflowValidationError> {
    let object = parameter_object(node)?;
    let body = required_string(object.get("body"), node, "try.body")?;
    let catch = optional_string(object.get("catch"));
    let finally = optional_string(object.get("finally"));
    Ok(TryParameters {
        body,
        catch,
        finally,
    })
}

pub fn parse_map_parameters(node: &WorkflowNode) -> Result<MapParameters, WorkflowValidationError> {
    let object = parameter_object(node)?;
    let items = object
        .get("items")
        .cloned()
        .ok_or_else(|| invalid_parameters(node, "map.items is required"))?;
    let target = required_string(object.get("target"), node, "map.target")?;
    let concurrency = object.get("concurrency").and_then(Value::as_i64);
    if concurrency.is_some_and(|value| value <= 0) {
        return Err(invalid_parameters(
            node,
            "map.concurrency must be greater than zero",
        ));
    }
    Ok(MapParameters {
        items,
        target,
        concurrency,
    })
}

pub fn parse_race_parameters(
    node: &WorkflowNode,
) -> Result<RaceParameters, WorkflowValidationError> {
    let object = parameter_object(node)?;
    let branches = string_array(object.get("branches"), node, "race.branches")?;
    if branches.is_empty() {
        return Err(invalid_parameters(node, "race.branches cannot be empty"));
    }
    let winner = BranchPolicy::parse(object.get("winner"), BranchPolicy::FirstSuccess)
        .map_err(|message| invalid_parameters(node, message))?;
    Ok(RaceParameters { branches, winner })
}

pub fn parse_emit_parameters(
    node: &WorkflowNode,
) -> Result<EmitParameters, WorkflowValidationError> {
    let object = parameter_object(node)?;
    let event_type = optional_string(object.get("event_type"));
    let data = object.get("data").cloned().unwrap_or(Value::Null);
    Ok(EmitParameters { event_type, data })
}

pub fn evaluate_switch(
    switch: &SwitchParameters,
    context: &Value,
) -> Result<Option<String>, WorkflowValidationError> {
    for case in &switch.cases {
        if evaluate_condition(&case.condition, context)? {
            return Ok(Some(case.target.clone()));
        }
    }
    Ok(switch.default.clone())
}

fn validate_graph_cycles(
    start: &str,
    nodes: &[WorkflowNode],
) -> Result<(), WorkflowValidationError> {
    let mut visited = HashSet::new();
    let mut stack = HashSet::new();
    let node_map: HashMap<_, _> = nodes.iter().map(|n| (&n.id, n)).collect();

    fn visit(
        id: &str,
        node_map: &HashMap<&String, &WorkflowNode>,
        visited: &mut HashSet<String>,
        stack: &mut HashSet<String>,
    ) -> Result<(), WorkflowValidationError> {
        if stack.contains(id) {
            return Err(WorkflowValidationError::RefCycle(id.to_string()));
        }
        if visited.contains(id) {
            return Ok(());
        }

        visited.insert(id.to_string());
        stack.insert(id.to_string());

        if let Some(node) = node_map.get(&id.to_string()) {
            for target in transition_targets(&node.transitions) {
                if stack.contains(&target)
                    && node_map.get(&target).is_some_and(|target_node| {
                        matches!(
                            target_node.kind,
                            WorkflowNodeKind::Try | WorkflowNodeKind::Map | WorkflowNodeKind::Race
                        )
                    })
                {
                    continue;
                }
                visit(&target, node_map, visited, stack)?;
            }
        }

        stack.remove(id);
        Ok(())
    }

    visit(start, &node_map, &mut visited, &mut stack)
}

pub fn resolve_value_refs(
    value: &Value,
    context: &Value,
) -> Result<Value, WorkflowValidationError> {
    match value {
        Value::Object(map) if map.len() == 1 && map.contains_key("$value") => {
            let raw = map
                .get("$value")
                .and_then(Value::as_str)
                .ok_or_else(|| WorkflowValidationError::InvalidValueRef(value.to_string()))?;
            resolve_value_ref(raw, context)
        }
        Value::Object(map) => {
            let mut resolved = Map::new();
            for (key, nested) in map {
                resolved.insert(key.clone(), resolve_value_refs(nested, context)?);
            }
            Ok(Value::Object(resolved))
        }
        Value::Array(items) => items
            .iter()
            .map(|item| resolve_value_refs(item, context))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        Value::String(raw) => resolve_template_string(raw, context),
        _ => Ok(value.clone()),
    }
}

pub fn evaluate_condition(
    condition: &Value,
    context: &Value,
) -> Result<bool, WorkflowValidationError> {
    if condition.is_null() {
        return Ok(true);
    }
    let Some(object) = condition.as_object() else {
        return Err(WorkflowValidationError::InvalidCondition(
            "condition must be an object".into(),
        ));
    };
    if let Some(all) = object.get("all") {
        let Some(items) = all.as_array() else {
            return Err(WorkflowValidationError::InvalidCondition(
                "all must be an array".into(),
            ));
        };
        for item in items {
            if !evaluate_condition(item, context)? {
                return Ok(false);
            }
        }
        return Ok(true);
    }
    if let Some(any) = object.get("any") {
        let Some(items) = any.as_array() else {
            return Err(WorkflowValidationError::InvalidCondition(
                "any must be an array".into(),
            ));
        };
        for item in items {
            if evaluate_condition(item, context)? {
                return Ok(true);
            }
        }
        return Ok(false);
    }
    if let Some(not) = object.get("not") {
        return Ok(!evaluate_condition(not, context)?);
    }

    let left = object
        .get("value")
        .or_else(|| object.get("left"))
        .ok_or_else(|| WorkflowValidationError::InvalidCondition("missing value".into()))?;
    let left = resolve_value_refs(left, context)?;
    if let Some(expected) = object.get("equals") {
        return Ok(left == resolve_value_refs(expected, context)?);
    }
    if let Some(expected) = object.get("not_equals") {
        return Ok(left != resolve_value_refs(expected, context)?);
    }
    if let Some(expected) = object.get("exists") {
        return Ok(expected.as_bool().unwrap_or(true) == !left.is_null());
    }
    Err(WorkflowValidationError::InvalidCondition(
        "expected equals, not_equals, exists, all, any, or not".into(),
    ))
}

pub fn next_transition(
    node: &WorkflowNode,
    status: WorkflowStatus,
    context: &Value,
) -> Result<Option<String>, WorkflowValidationError> {
    for branch in &node.transitions.branches {
        if evaluate_condition(&branch.when, context)? {
            return Ok(Some(branch.target.clone()));
        }
    }
    let target = match status {
        WorkflowStatus::Succeeded => node
            .transitions
            .on_success
            .as_ref()
            .or(node.transitions.next.as_ref()),
        WorkflowStatus::Failed | WorkflowStatus::Blocked => node.transitions.on_failure.as_ref(),
        WorkflowStatus::TimedOut => node.transitions.on_timeout.as_ref(),
        WorkflowStatus::Canceled => None,
        _ => node.transitions.next.as_ref(),
    };
    Ok(target.cloned())
}

fn validate_condition(condition: &Value) -> Result<(), WorkflowValidationError> {
    if condition.is_null() || condition.is_object() {
        Ok(())
    } else {
        Err(WorkflowValidationError::InvalidCondition(
            "condition must be an object".into(),
        ))
    }
}

fn validate_control_node_parameters(node: &WorkflowNode) -> Result<(), WorkflowValidationError> {
    match node.kind {
        WorkflowNodeKind::Switch => {
            let params = parse_switch_parameters(node)?;
            for case in params.cases {
                validate_condition(&case.condition)?;
            }
        }
        WorkflowNodeKind::Parallel => {
            parse_parallel_parameters(node)?;
        }
        WorkflowNodeKind::Join => {
            parse_join_parameters(node)?;
        }
        WorkflowNodeKind::Try => {
            parse_try_parameters(node)?;
        }
        WorkflowNodeKind::Map => {
            parse_map_parameters(node)?;
        }
        WorkflowNodeKind::Race => {
            parse_race_parameters(node)?;
        }
        WorkflowNodeKind::Emit => {
            parse_emit_parameters(node)?;
        }
        _ => {}
    }
    Ok(())
}

fn parameter_targets(node: &WorkflowNode) -> Result<Vec<String>, WorkflowValidationError> {
    let mut targets = Vec::new();
    match node.kind {
        WorkflowNodeKind::Switch => {
            let params = parse_switch_parameters(node)?;
            targets.extend(params.cases.into_iter().map(|case| case.target));
            targets.extend(params.default);
        }
        WorkflowNodeKind::Parallel => {
            targets.extend(parse_parallel_parameters(node)?.branches);
        }
        WorkflowNodeKind::Join => {
            targets.extend(parse_join_parameters(node)?.wait_for);
        }
        WorkflowNodeKind::Try => {
            let params = parse_try_parameters(node)?;
            targets.push(params.body);
            targets.extend(params.catch);
            targets.extend(params.finally);
        }
        WorkflowNodeKind::Map => {
            targets.push(parse_map_parameters(node)?.target);
        }
        WorkflowNodeKind::Race => {
            targets.extend(parse_race_parameters(node)?.branches);
        }
        _ => {}
    }
    Ok(targets)
}

fn transition_targets(transitions: &WorkflowTransitions) -> Vec<String> {
    let mut targets = Vec::new();
    for target in [
        &transitions.next,
        &transitions.on_success,
        &transitions.on_failure,
        &transitions.on_timeout,
        &transitions.on_reject,
    ]
    .into_iter()
    .flatten()
    {
        targets.push(target.clone());
    }
    for branch in &transitions.branches {
        targets.push(branch.target.clone());
    }
    targets
}

fn parameter_object(node: &WorkflowNode) -> Result<&Map<String, Value>, WorkflowValidationError> {
    node.parameters
        .as_object()
        .ok_or_else(|| invalid_parameters(node, "parameters must be an object"))
}

fn required_string(
    value: Option<&Value>,
    node: &WorkflowNode,
    label: &str,
) -> Result<String, WorkflowValidationError> {
    value
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| invalid_parameters(node, format!("{label} is required")))
}

fn optional_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn string_array(
    value: Option<&Value>,
    node: &WorkflowNode,
    label: &str,
) -> Result<Vec<String>, WorkflowValidationError> {
    let items = value
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_parameters(node, format!("{label} must be an array")))?;
    items
        .iter()
        .map(|item| {
            item.as_str()
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .ok_or_else(|| invalid_parameters(node, format!("{label} must contain strings")))
        })
        .collect()
}

fn invalid_parameters(node: &WorkflowNode, message: impl Into<String>) -> WorkflowValidationError {
    WorkflowValidationError::InvalidNodeParameters {
        node: node.id.clone(),
        message: message.into(),
    }
}

fn normalize_layout(root: &mut Map<String, Value>) {
    let Some(ui) = root.get_mut("ui").and_then(Value::as_object_mut) else {
        return;
    };
    let Some(layout) = ui.get_mut("layout").and_then(Value::as_object_mut) else {
        return;
    };
    let direct_nodes = layout
        .iter()
        .filter_map(|(key, value)| {
            if key == "nodes" {
                return None;
            }
            value.as_object()?;
            Some((key.clone(), value.clone()))
        })
        .collect::<Vec<_>>();
    if direct_nodes.is_empty() {
        return;
    }
    for (id, _) in &direct_nodes {
        layout.remove(id);
    }
    let nodes = layout
        .entry("nodes")
        .or_insert_with(|| Value::Object(Map::new()));
    if !nodes.is_object() {
        *nodes = Value::Object(Map::new());
    }
    let Some(nodes) = nodes.as_object_mut() else {
        return;
    };
    for (id, position) in direct_nodes {
        nodes.entry(id.clone()).or_insert(position);
    }
}

fn node_ids(nodes: &[Value]) -> HashSet<String> {
    nodes
        .iter()
        .filter_map(|node| node.get("id").and_then(Value::as_str).map(str::to_string))
        .collect()
}

fn ensure_end_node(nodes: &mut Vec<Value>, ids: &mut HashSet<String>) -> String {
    if let Some(id) = first_node_id(nodes, |kind| kind == Some("end")) {
        return id;
    }
    let id = unique_node_id("end", ids);
    nodes.push(serde_json::json!({ "id": id, "kind": "end" }));
    id
}

fn ensure_start_node(
    nodes: &mut Vec<Value>,
    ids: &mut HashSet<String>,
    previous_start: &str,
    end_id: &str,
) -> String {
    if let Some(id) = first_node_id(nodes, |kind| kind == Some("start")) {
        let fallback = if id == previous_start {
            end_id
        } else {
            previous_start
        };
        if let Some(node) = nodes
            .iter_mut()
            .find(|node| node_id(node).as_deref() == Some(id.as_str()))
        {
            ensure_next_transition(node, fallback);
        }
        return id;
    }

    let id = unique_node_id("start", ids);
    let target = if previous_start == id.as_str() {
        end_id
    } else {
        previous_start
    };
    nodes.insert(
        0,
        serde_json::json!({
            "id": id,
            "kind": "start",
            "transitions": { "next": target }
        }),
    );
    id
}

fn route_success_terminals_to_end(nodes: &mut [Value], end_id: &str) {
    for node in nodes {
        if node_kind(node).as_deref() == Some("end") {
            continue;
        }
        if has_success_transition(node) {
            continue;
        }
        ensure_next_transition(node, end_id);
    }
}

fn ensure_next_transition(node: &mut Value, target: &str) {
    let Some(node) = node.as_object_mut() else {
        return;
    };
    let transitions = node
        .entry("transitions")
        .or_insert_with(|| Value::Object(Map::new()));
    let Some(transitions) = transitions.as_object_mut() else {
        return;
    };
    transitions
        .entry("next")
        .or_insert_with(|| Value::String(target.to_string()));
}

fn has_success_transition(node: &Value) -> bool {
    let Some(transitions) = node.get("transitions").and_then(Value::as_object) else {
        return false;
    };
    ["next", "on_success"]
        .into_iter()
        .any(|key| non_empty_string(transitions.get(key)))
        || transitions
            .get("branches")
            .and_then(Value::as_array)
            .is_some_and(|branches| !branches.is_empty())
}

fn first_node_id(nodes: &[Value], predicate: impl Fn(Option<&str>) -> bool) -> Option<String> {
    nodes.iter().find_map(|node| {
        if predicate(node_kind(node).as_deref()) {
            return node_id(node);
        }
        None
    })
}

fn node_id(node: &Value) -> Option<String> {
    node.get("id").and_then(Value::as_str).map(str::to_string)
}

fn node_kind(node: &Value) -> Option<String> {
    node.get("kind").and_then(Value::as_str).map(str::to_string)
}

fn node_kind_by_id(nodes: &[Value], id: &str) -> Option<String> {
    nodes
        .iter()
        .find(|node| node_id(node).as_deref() == Some(id))
        .and_then(node_kind)
}

fn non_empty_string(value: Option<&Value>) -> bool {
    value
        .and_then(Value::as_str)
        .is_some_and(|value| !value.is_empty())
}

fn unique_node_id(base: &str, ids: &mut HashSet<String>) -> String {
    if ids.insert(base.to_string()) {
        return base.to_string();
    }
    for index in 2.. {
        let candidate = format!("{base}_{index}");
        if ids.insert(candidate.clone()) {
            return candidate;
        }
    }
    unreachable!("node id generation should always find an unused id")
}

fn expand_refs_in_value(
    value: &mut Value,
    defs: &Value,
    stack: &mut Vec<String>,
) -> Result<(), WorkflowValidationError> {
    match value {
        Value::Object(map) => {
            if let Some(reference) = map.get("$ref").and_then(Value::as_str).map(str::to_string) {
                if let Some(pointer) = reference.strip_prefix("#/$defs/") {
                    if stack.iter().any(|item| item == &reference) {
                        return Err(WorkflowValidationError::RefCycle(reference));
                    }
                    let path = format!("/{pointer}");
                    let mut replacement = defs
                        .pointer(&path)
                        .cloned()
                        .ok_or_else(|| WorkflowValidationError::MissingRef(reference.clone()))?;
                    stack.push(reference.clone());
                    expand_refs_in_value(&mut replacement, defs, stack)?;
                    stack.pop();
                    for (key, overlay) in map.clone() {
                        if key != "$ref" && key != "with" {
                            if let Value::Object(replacement_map) = &mut replacement {
                                replacement_map.insert(key, overlay);
                            }
                        }
                    }
                    if let Some(with) = map.get("with") {
                        merge_overlay(&mut replacement, with.clone());
                    }
                    *value = replacement;
                    return Ok(());
                }
                if reference.starts_with("runinator://") {
                    return Ok(());
                }
                return Err(WorkflowValidationError::MissingRef(reference));
            }
            for nested in map.values_mut() {
                expand_refs_in_value(nested, defs, stack)?;
            }
        }
        Value::Array(items) => {
            for item in items {
                expand_refs_in_value(item, defs, stack)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn merge_overlay(target: &mut Value, overlay: Value) {
    match (target, overlay) {
        (Value::Object(target), Value::Object(overlay)) => {
            for (key, value) in overlay {
                match target.get_mut(&key) {
                    Some(existing) => merge_overlay(existing, value),
                    None => {
                        target.insert(key, value);
                    }
                }
            }
        }
        (target, overlay) => *target = overlay,
    }
}

fn resolve_value_ref(raw: &str, context: &Value) -> Result<Value, WorkflowValidationError> {
    let Some((scope, pointer)) = raw.split_once('#') else {
        return Err(WorkflowValidationError::InvalidValueRef(raw.into()));
    };
    let pointer = if pointer.is_empty() { "" } else { pointer };
    if !pointer.is_empty() && !pointer.starts_with('/') {
        return Err(WorkflowValidationError::InvalidValueRef(raw.into()));
    }
    let base = context
        .pointer(&format!("/{}", scope.replace('.', "/")))
        .ok_or_else(|| WorkflowValidationError::InvalidValueRef(raw.into()))?;
    Ok(base.pointer(pointer).cloned().unwrap_or(Value::Null))
}

fn resolve_template_string(raw: &str, context: &Value) -> Result<Value, WorkflowValidationError> {
    let trimmed = raw.trim();
    if let Some(reference) = trimmed
        .strip_prefix("{{")
        .and_then(|value| value.strip_suffix("}}"))
        .map(str::trim)
        .filter(|reference| !reference.is_empty())
    {
        if trimmed.len() == raw.len() {
            return resolve_value_ref(reference, context);
        }
    }

    let mut rendered = String::new();
    let mut remaining = raw;
    loop {
        let Some(start) = remaining.find("{{") else {
            rendered.push_str(remaining);
            break;
        };
        rendered.push_str(&remaining[..start]);
        let after_start = &remaining[start + 2..];
        let Some(end) = after_start.find("}}") else {
            rendered.push_str(&remaining[start..]);
            break;
        };
        let reference = after_start[..end].trim();
        if reference.is_empty() {
            return Err(WorkflowValidationError::InvalidValueRef(raw.into()));
        }
        let value = resolve_value_ref(reference, context)?;
        rendered.push_str(&template_value_to_string(value));
        remaining = &after_start[end + 2..];
    }
    Ok(Value::String(rendered))
}

fn template_value_to_string(value: Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(value) => value,
        other => other.to_string(),
    }
}

pub fn outputs_context(parameters: &Value, outputs: &HashMap<String, Value>) -> Value {
    let mut steps = Map::new();
    for (node, output) in outputs {
        steps.insert(node.clone(), serde_json::json!({ "output": output }));
    }
    serde_json::json!({
        "input": parameters,
        "steps": steps
    })
}

#[cfg(test)]
mod tests;
