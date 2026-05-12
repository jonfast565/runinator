use std::collections::{HashMap, HashSet};

use runinator_models::workflows::{
    WorkflowDefinition, WorkflowNode, WorkflowNodeKind, WorkflowNodeRef, WorkflowStatus,
    WorkflowTransitions,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkflowPathSegment {
    Key(String),
    Index(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkflowRefSource {
    Input,
    Prev,
    Workflow,
    NodeOutput(WorkflowNodeRef),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowValueRef {
    pub source: WorkflowRefSource,
    pub path: Vec<WorkflowPathSegment>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WorkflowExpression {
    Literal(Value),
    Ref(WorkflowValueRef),
    Concat(Vec<WorkflowExpression>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct SwitchCase {
    pub target: WorkflowNodeRef,
    pub condition: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SwitchParameters {
    pub value: Value,
    pub cases: Vec<SwitchCase>,
    pub default: Option<WorkflowNodeRef>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParallelParameters {
    pub branches: Vec<WorkflowNodeRef>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct JoinParameters {
    pub wait_for: Vec<WorkflowNodeRef>,
    pub mode: BranchPolicy,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TryParameters {
    pub body: WorkflowNodeRef,
    pub catch: Option<WorkflowNodeRef>,
    pub finally: Option<WorkflowNodeRef>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MapParameters {
    pub items: Value,
    pub target: WorkflowNodeRef,
    pub concurrency: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RaceParameters {
    pub branches: Vec<WorkflowNodeRef>,
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
            if !seen.insert(node.id.as_str().to_string()) {
                return Err(WorkflowValidationError::DuplicateNode(
                    node.id.as_str().to_string(),
                ));
            }
            Ok(node.id.as_str().to_string())
        })
        .collect::<Result<HashSet<_>, _>>()?;

    if !ids.contains(&start) {
        return Err(WorkflowValidationError::MissingStartNode(start));
    }
    if nodes
        .iter()
        .find(|node| node.id.as_str() == start)
        .is_none_or(|node| node.kind != WorkflowNodeKind::Start)
    {
        return Err(WorkflowValidationError::MissingStartKind);
    }
    if !nodes.iter().any(|node| node.kind == WorkflowNodeKind::End) {
        return Err(WorkflowValidationError::MissingEndNode);
    }

    for node in &nodes {
        if node.kind == WorkflowNodeKind::Task && node.task_id.is_none() {
            return Err(WorkflowValidationError::MissingTaskId(
                node.id.as_str().to_string(),
            ));
        }
        if node.retry.max_attempts <= 0 {
            return Err(WorkflowValidationError::InvalidRetry(
                node.id.as_str().to_string(),
            ));
        }
        if node.timeout_seconds.is_some_and(|timeout| timeout <= 0) {
            return Err(WorkflowValidationError::InvalidTimeout(
                node.id.as_str().to_string(),
            ));
        }
        if node.max_iterations.is_some_and(|limit| limit <= 0) {
            return Err(WorkflowValidationError::InvalidLoopLimit(
                node.id.as_str().to_string(),
            ));
        }
        validate_condition(&node.condition)?;
        validate_control_node_parameters(node)?;
        for target in transition_targets(&node.transitions) {
            if !ids.contains(target.as_str()) {
                return Err(WorkflowValidationError::MissingTransition {
                    node: node.id.as_str().to_string(),
                    target: target.into_string(),
                });
            }
        }
        for target in parameter_targets(node)? {
            if !ids.contains(target.as_str()) {
                return Err(WorkflowValidationError::MissingTransition {
                    node: node.id.as_str().to_string(),
                    target: target.into_string(),
                });
            }
        }
        for reference in value_refs(node)? {
            if let WorkflowRefSource::NodeOutput(target) = reference.source {
                if !ids.contains(target.as_str()) {
                    return Err(WorkflowValidationError::MissingTransition {
                        node: node.id.as_str().to_string(),
                        target: target.into_string(),
                    });
                }
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
            let target = required_node_ref(case_object.get("target"), node, "switch case target")?;
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
        .map(|value| parse_node_ref_value(value, Some(node), "switch.default"))
        .transpose()?;
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
    let branches = node_ref_array(object.get("branches"), node, "parallel.branches")?;
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
    let wait_for = node_ref_array(object.get("wait_for"), node, "join.wait_for")?;
    if wait_for.is_empty() {
        return Err(invalid_parameters(node, "join.wait_for cannot be empty"));
    }
    let mode = BranchPolicy::parse(object.get("mode"), BranchPolicy::All)
        .map_err(|message| invalid_parameters(node, message))?;
    Ok(JoinParameters { wait_for, mode })
}

pub fn parse_try_parameters(node: &WorkflowNode) -> Result<TryParameters, WorkflowValidationError> {
    let object = parameter_object(node)?;
    let body = required_node_ref(object.get("body"), node, "try.body")?;
    let catch = optional_node_ref(object.get("catch"), node, "try.catch")?;
    let finally = optional_node_ref(object.get("finally"), node, "try.finally")?;
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
    let target = required_node_ref(object.get("target"), node, "map.target")?;
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
    let branches = node_ref_array(object.get("branches"), node, "race.branches")?;
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
            return Ok(Some(case.target.as_str().to_string()));
        }
    }
    Ok(switch
        .default
        .as_ref()
        .map(|target| target.as_str().to_string()))
}

fn validate_graph_cycles(
    start: &str,
    nodes: &[WorkflowNode],
) -> Result<(), WorkflowValidationError> {
    let mut visited = HashSet::new();
    let mut stack = HashSet::new();
    let node_map: HashMap<_, _> = nodes.iter().map(|n| (n.id.as_str(), n)).collect();

    fn visit(
        id: &str,
        node_map: &HashMap<&str, &WorkflowNode>,
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

        if let Some(node) = node_map.get(id) {
            for target in transition_targets(&node.transitions) {
                if stack.contains(target.as_str())
                    && node_map.get(target.as_str()).is_some_and(|target_node| {
                        matches!(
                            target_node.kind,
                            WorkflowNodeKind::Try | WorkflowNodeKind::Map | WorkflowNodeKind::Race
                        )
                    })
                {
                    continue;
                }
                visit(target.as_str(), node_map, visited, stack)?;
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
    let expression = parse_expression(value)?;
    evaluate_expression(&expression, context)
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
            return Ok(Some(branch.target.as_str().to_string()));
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
    Ok(target.map(|target| target.as_str().to_string()))
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

fn parameter_targets(node: &WorkflowNode) -> Result<Vec<WorkflowNodeRef>, WorkflowValidationError> {
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

fn value_refs(node: &WorkflowNode) -> Result<Vec<WorkflowValueRef>, WorkflowValidationError> {
    let mut refs = Vec::new();
    collect_value_refs(&node.parameters, &mut refs)?;
    collect_value_refs(&node.wait, &mut refs)?;
    collect_value_refs(&node.condition, &mut refs)?;
    for branch in &node.transitions.branches {
        collect_value_refs(&branch.when, &mut refs)?;
    }
    Ok(refs)
}

fn collect_value_refs(
    value: &Value,
    refs: &mut Vec<WorkflowValueRef>,
) -> Result<(), WorkflowValidationError> {
    match value {
        Value::Object(map) if map.contains_key("$value") => {
            return Err(WorkflowValidationError::InvalidValueRef(value.to_string()));
        }
        Value::Object(map)
            if map.contains_key("$ref")
                || map.contains_key("$concat")
                || map.contains_key("$literal")
                || map.contains_key("$node") =>
        {
            if map.len() != 1 {
                return Err(WorkflowValidationError::InvalidValueRef(value.to_string()));
            }
            if let Some(reference) = map.get("$ref") {
                refs.push(parse_value_ref(reference)?);
            } else if let Some(items) = map.get("$concat") {
                let items = items
                    .as_array()
                    .ok_or_else(|| WorkflowValidationError::InvalidValueRef(value.to_string()))?;
                for item in items {
                    collect_value_refs(item, refs)?;
                }
            }
        }
        Value::Object(map) => {
            for nested in map.values() {
                collect_value_refs(nested, refs)?;
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_value_refs(item, refs)?;
            }
        }
        Value::String(raw) if raw.contains("{{") || raw.contains("}}") => {
            return Err(WorkflowValidationError::InvalidValueRef(raw.clone()));
        }
        _ => {}
    }
    Ok(())
}

fn transition_targets(transitions: &WorkflowTransitions) -> Vec<WorkflowNodeRef> {
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
        targets.push((*target).clone());
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

fn optional_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn required_node_ref(
    value: Option<&Value>,
    node: &WorkflowNode,
    label: &str,
) -> Result<WorkflowNodeRef, WorkflowValidationError> {
    let value = value.ok_or_else(|| invalid_parameters(node, format!("{label} is required")))?;
    parse_node_ref_value(value, Some(node), label)
}

fn optional_node_ref(
    value: Option<&Value>,
    node: &WorkflowNode,
    label: &str,
) -> Result<Option<WorkflowNodeRef>, WorkflowValidationError> {
    value
        .map(|value| parse_node_ref_value(value, Some(node), label))
        .transpose()
}

fn parse_node_ref_value(
    value: &Value,
    node: Option<&WorkflowNode>,
    label: &str,
) -> Result<WorkflowNodeRef, WorkflowValidationError> {
    let invalid = || {
        if let Some(node) = node {
            invalid_parameters(
                node,
                format!("{label} must be {{ \"$node\": \"node_id\" }}"),
            )
        } else {
            WorkflowValidationError::InvalidValueRef(value.to_string())
        }
    };
    let object = value.as_object().ok_or_else(invalid)?;
    if object.len() != 1 || !object.contains_key("$node") {
        return Err(invalid());
    }
    let target = object
        .get("$node")
        .and_then(Value::as_str)
        .filter(|target| !target.is_empty())
        .ok_or_else(invalid)?;
    Ok(WorkflowNodeRef::new(target))
}

fn node_ref_array(
    value: Option<&Value>,
    node: &WorkflowNode,
    label: &str,
) -> Result<Vec<WorkflowNodeRef>, WorkflowValidationError> {
    let items = value
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_parameters(node, format!("{label} must be an array")))?;
    items
        .iter()
        .map(|item| parse_node_ref_value(item, Some(node), label))
        .collect()
}

fn invalid_parameters(node: &WorkflowNode, message: impl Into<String>) -> WorkflowValidationError {
    WorkflowValidationError::InvalidNodeParameters {
        node: node.id.as_str().to_string(),
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
            "transitions": { "next": { "$node": target } }
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
        .or_insert_with(|| serde_json::json!({ "$node": target }));
}

fn has_success_transition(node: &Value) -> bool {
    let Some(transitions) = node.get("transitions").and_then(Value::as_object) else {
        return false;
    };
    ["next", "on_success"]
        .into_iter()
        .any(|key| valid_node_ref_value(transitions.get(key)))
        || transitions
            .get("branches")
            .and_then(Value::as_array)
            .is_some_and(|branches| !branches.is_empty())
}

fn valid_node_ref_value(value: Option<&Value>) -> bool {
    value.is_some_and(|value| parse_node_ref_value(value, None, "transition").is_ok())
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

fn parse_expression(value: &Value) -> Result<WorkflowExpression, WorkflowValidationError> {
    match value {
        Value::Object(map) if map.contains_key("$value") => {
            Err(WorkflowValidationError::InvalidValueRef(value.to_string()))
        }
        Value::Object(map)
            if map.contains_key("$ref")
                || map.contains_key("$concat")
                || map.contains_key("$literal")
                || map.contains_key("$node") =>
        {
            if map.len() != 1 {
                return Err(WorkflowValidationError::InvalidValueRef(value.to_string()));
            }
            if let Some(reference) = map.get("$ref") {
                return Ok(WorkflowExpression::Ref(parse_value_ref(reference)?));
            }
            if let Some(items) = map.get("$concat") {
                let items = items
                    .as_array()
                    .ok_or_else(|| WorkflowValidationError::InvalidValueRef(value.to_string()))?;
                return Ok(WorkflowExpression::Concat(
                    items
                        .iter()
                        .map(parse_expression)
                        .collect::<Result<Vec<_>, _>>()?,
                ));
            }
            if let Some(literal) = map.get("$literal") {
                return Ok(WorkflowExpression::Literal(literal.clone()));
            }
            Err(WorkflowValidationError::InvalidValueRef(value.to_string()))
        }
        Value::Object(map) => {
            let mut resolved = Map::new();
            for (key, nested) in map {
                resolved.insert(
                    key.clone(),
                    evaluate_static_expression(parse_expression(nested)?)?,
                );
            }
            Ok(WorkflowExpression::Literal(Value::Object(resolved)))
        }
        Value::Array(items) => Ok(WorkflowExpression::Literal(Value::Array(
            items
                .iter()
                .map(|item| evaluate_static_expression(parse_expression(item)?))
                .collect::<Result<Vec<_>, _>>()?,
        ))),
        Value::String(raw) if raw.contains("{{") || raw.contains("}}") => {
            Err(WorkflowValidationError::InvalidValueRef(raw.clone()))
        }
        _ => Ok(WorkflowExpression::Literal(value.clone())),
    }
}

fn evaluate_static_expression(
    expression: WorkflowExpression,
) -> Result<Value, WorkflowValidationError> {
    match expression {
        WorkflowExpression::Literal(value) => Ok(value),
        WorkflowExpression::Ref(reference) => Ok(Value::Object(Map::from_iter([(
            "$ref".into(),
            serialize_value_ref(&reference),
        )]))),
        WorkflowExpression::Concat(items) => Ok(Value::Object(Map::from_iter([(
            "$concat".into(),
            Value::Array(
                items
                    .into_iter()
                    .map(evaluate_static_expression)
                    .collect::<Result<Vec<_>, _>>()?,
            ),
        )]))),
    }
}

fn evaluate_expression(
    expression: &WorkflowExpression,
    context: &Value,
) -> Result<Value, WorkflowValidationError> {
    match expression {
        WorkflowExpression::Literal(value) => match value {
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
            _ => Ok(value.clone()),
        },
        WorkflowExpression::Ref(reference) => resolve_value_ref(reference, context),
        WorkflowExpression::Concat(items) => {
            let mut rendered = String::new();
            for item in items {
                rendered.push_str(&template_value_to_string(evaluate_expression(
                    item, context,
                )?));
            }
            Ok(Value::String(rendered))
        }
    }
}

fn parse_value_ref(value: &Value) -> Result<WorkflowValueRef, WorkflowValidationError> {
    let object = value
        .as_object()
        .ok_or_else(|| WorkflowValidationError::InvalidValueRef(value.to_string()))?;
    if object.len() != 1
        && !(object.len() == 2 && object.contains_key("node") && object.contains_key("output"))
    {
        return Err(WorkflowValidationError::InvalidValueRef(value.to_string()));
    }
    if let Some(path) = object.get("input") {
        return Ok(WorkflowValueRef {
            source: WorkflowRefSource::Input,
            path: parse_path(path)?,
        });
    }
    if let Some(path) = object.get("prev") {
        return Ok(WorkflowValueRef {
            source: WorkflowRefSource::Prev,
            path: parse_path(path)?,
        });
    }
    if let Some(path) = object.get("workflow") {
        return Ok(WorkflowValueRef {
            source: WorkflowRefSource::Workflow,
            path: parse_path(path)?,
        });
    }
    if let (Some(node), Some(output)) = (object.get("node"), object.get("output")) {
        let node = node
            .as_str()
            .filter(|node| !node.is_empty())
            .ok_or_else(|| WorkflowValidationError::InvalidValueRef(value.to_string()))?;
        return Ok(WorkflowValueRef {
            source: WorkflowRefSource::NodeOutput(WorkflowNodeRef::new(node)),
            path: parse_path(output)?,
        });
    }
    Err(WorkflowValidationError::InvalidValueRef(value.to_string()))
}

fn parse_path(value: &Value) -> Result<Vec<WorkflowPathSegment>, WorkflowValidationError> {
    let items = value
        .as_array()
        .ok_or_else(|| WorkflowValidationError::InvalidValueRef(value.to_string()))?;
    items
        .iter()
        .map(|item| {
            if let Some(key) = item.as_str() {
                return Ok(WorkflowPathSegment::Key(key.to_string()));
            }
            if let Some(index) = item.as_u64() {
                return usize::try_from(index)
                    .map(WorkflowPathSegment::Index)
                    .map_err(|_| WorkflowValidationError::InvalidValueRef(value.to_string()));
            }
            Err(WorkflowValidationError::InvalidValueRef(value.to_string()))
        })
        .collect()
}

fn resolve_value_ref(
    reference: &WorkflowValueRef,
    context: &Value,
) -> Result<Value, WorkflowValidationError> {
    let base = match &reference.source {
        WorkflowRefSource::Input => context.get("input"),
        WorkflowRefSource::Prev => context.get("prev"),
        WorkflowRefSource::Workflow => context.get("workflow"),
        WorkflowRefSource::NodeOutput(node) => context
            .get("steps")
            .and_then(|steps| steps.get(node.as_str()))
            .and_then(|step| step.get("output")),
    }
    .ok_or_else(|| {
        WorkflowValidationError::InvalidValueRef(serialize_value_ref(reference).to_string())
    })?;
    Ok(resolve_path(base, &reference.path)
        .cloned()
        .unwrap_or(Value::Null))
}

fn resolve_path<'a>(value: &'a Value, path: &[WorkflowPathSegment]) -> Option<&'a Value> {
    let mut current = value;
    for segment in path {
        current = match segment {
            WorkflowPathSegment::Key(key) => current.get(key)?,
            WorkflowPathSegment::Index(index) => current.get(*index)?,
        };
    }
    Some(current)
}

fn serialize_value_ref(reference: &WorkflowValueRef) -> Value {
    let path = Value::Array(
        reference
            .path
            .iter()
            .map(|segment| match segment {
                WorkflowPathSegment::Key(key) => Value::String(key.clone()),
                WorkflowPathSegment::Index(index) => Value::from(*index),
            })
            .collect(),
    );
    match &reference.source {
        WorkflowRefSource::Input => serde_json::json!({ "input": path }),
        WorkflowRefSource::Prev => serde_json::json!({ "prev": path }),
        WorkflowRefSource::Workflow => serde_json::json!({ "workflow": path }),
        WorkflowRefSource::NodeOutput(node) => {
            serde_json::json!({ "node": node.as_str(), "output": path })
        }
    }
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
