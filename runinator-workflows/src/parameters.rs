use runinator_models::value::{Map, Value};
use runinator_models::workflows::{
    WorkflowNode, WorkflowNodeKind, WorkflowNodeRef, WorkflowStatus, WorkflowWaitSeconds,
};

use crate::conditions::{evaluate_condition, validate_condition};
use crate::errors::WorkflowValidationError;
use crate::expressions::parse_value_ref;
use crate::keys::{COND_EQUALS, COND_EXISTS, COND_NOT_EQUALS, COND_VALUE};
use crate::types::{
    ApprovalParameters, BranchPolicy, DeliverableItem, DeliverableParameters, GateParameters,
    InputParameters, JoinParameters, LoopParameters, MapParameters, OutputParameters,
    ParallelParameters, RaceParameters, SignalParameters, SwitchCase, SwitchParameters,
    TryParameters, WaitParameters, WorkflowValueRef,
};
use runinator_models::orchestration::GateKind;

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
                condition.insert(COND_VALUE.into(), value.clone());
                for key in [COND_EQUALS, COND_NOT_EQUALS, COND_EXISTS] {
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

pub fn parse_output_parameters(
    node: &WorkflowNode,
) -> Result<OutputParameters, WorkflowValidationError> {
    let object = parameter_object(node)?;
    let event_type = optional_string(object.get("event_type"));
    let data = object.get("data").cloned().unwrap_or(Value::Null);
    Ok(OutputParameters { event_type, data })
}

pub fn parse_deliverable_parameters(
    node: &WorkflowNode,
) -> Result<DeliverableParameters, WorkflowValidationError> {
    let object = parameter_object(node)?;
    let entries = object
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_parameters(node, "deliverable.items must be an array"))?;
    if entries.is_empty() {
        return Err(invalid_parameters(
            node,
            "deliverable.items cannot be empty",
        ));
    }
    let mut items = Vec::with_capacity(entries.len());
    for entry in entries {
        let entry = entry
            .as_object()
            .ok_or_else(|| invalid_parameters(node, "deliverable.items entry must be an object"))?;
        let name = entry
            .get("name")
            .and_then(Value::as_str)
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty())
            .ok_or_else(|| invalid_parameters(node, "deliverable item requires a name"))?;
        let source = entry
            .get("source")
            .cloned()
            .ok_or_else(|| invalid_parameters(node, "deliverable item requires a source"))?;
        items.push(DeliverableItem { name, source });
    }
    Ok(DeliverableParameters { items })
}

pub fn parse_input_parameters(node: &WorkflowNode) -> InputParameters {
    let object = parameter_object(node);
    let prompt = object
        .ok()
        .and_then(|object| object.get("prompt"))
        .and_then(Value::as_str)
        .map(|prompt| prompt.trim().to_string())
        .filter(|prompt| !prompt.is_empty());
    InputParameters { prompt }
}

/// parse a wait node's `wait` config. all fields default, so non-object configs are tolerated.
pub fn parse_wait_parameters(node: &WorkflowNode) -> WaitParameters {
    let seconds = match node.wait.seconds.as_ref() {
        Some(WorkflowWaitSeconds::Integer(value)) => (*value).max(0),
        Some(WorkflowWaitSeconds::Expression(_)) | None => 0,
    };
    let until_status = node.wait.until_status.clone();
    let initial_status = node
        .wait
        .initial_status
        .clone()
        .unwrap_or_else(|| WorkflowStatus::Waiting.as_str().to_string());
    WaitParameters {
        seconds,
        until_status,
        initial_status,
    }
}

/// parse an approval node's parameters. carries the raw parameters along as approval metadata.
pub fn parse_approval_parameters(node: &WorkflowNode) -> ApprovalParameters {
    let approval_type = node
        .parameters
        .get("approval_type")
        .and_then(Value::as_str)
        .unwrap_or("generic")
        .to_string();
    let prompt = node
        .parameters
        .get("prompt")
        .and_then(Value::as_str)
        .unwrap_or("Approval required")
        .to_string();
    ApprovalParameters {
        approval_type,
        prompt,
        metadata: node.parameters.clone().into(),
    }
}

/// parse a signal node's parameters. `name` is the signal the node parks on.
pub fn parse_signal_parameters(node: &WorkflowNode) -> SignalParameters {
    let name = node
        .parameters
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    SignalParameters { name }
}

/// default seconds between gate re-checks while it stays closed.
const DEFAULT_GATE_POLL_SECONDS: i64 = 30;

/// parse a gate node's parameters. `kind` selects the resolver (manual/condition/external);
/// `when` holds the condition for condition gates; `poll_interval`/`timeout` tune the poll loop.
pub fn parse_gate_parameters(node: &WorkflowNode) -> GateParameters {
    let kind = match node.parameters.get("kind").and_then(Value::as_str) {
        Some("condition") => GateKind::Condition,
        Some("external") => GateKind::External,
        _ => GateKind::Manual,
    };
    let condition = node.parameters.get("when").cloned().unwrap_or(Value::Null);
    let poll_interval_seconds = node
        .parameters
        .get("poll_interval")
        .and_then(Value::as_i64)
        .filter(|seconds| *seconds > 0)
        .unwrap_or(DEFAULT_GATE_POLL_SECONDS);
    let deadline_seconds = node
        .parameters
        .get("timeout")
        .and_then(Value::as_i64)
        .filter(|seconds| *seconds > 0);
    let label = node
        .parameters
        .get("label")
        .and_then(Value::as_str)
        .map(str::to_string);
    GateParameters {
        kind,
        condition,
        poll_interval_seconds,
        deadline_seconds,
        label,
        metadata: node.parameters.clone().into(),
    }
}

/// extract a loop node's iteration items from its runtime-resolved parameters.
pub fn parse_loop_items(resolved_parameters: &Value) -> LoopParameters {
    let items = resolved_parameters
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    LoopParameters { items }
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

pub(crate) fn validate_control_node_parameters(
    node: &WorkflowNode,
) -> Result<(), WorkflowValidationError> {
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
        WorkflowNodeKind::Output => {
            parse_output_parameters(node)?;
        }
        WorkflowNodeKind::Deliverable => {
            parse_deliverable_parameters(node)?;
        }
        WorkflowNodeKind::Input => {
            let _ = parse_input_parameters(node);
        }
        _ => {}
    }
    Ok(())
}

pub(crate) fn parameter_targets(
    node: &WorkflowNode,
) -> Result<Vec<WorkflowNodeRef>, WorkflowValidationError> {
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

pub(crate) fn parameter_object(node: &WorkflowNode) -> Result<&Map, WorkflowValidationError> {
    node.parameters
        .as_object()
        .ok_or_else(|| invalid_parameters(node, "parameters must be an object"))
}

pub(crate) fn optional_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub(crate) fn required_node_ref(
    value: Option<&Value>,
    node: &WorkflowNode,
    label: &str,
) -> Result<WorkflowNodeRef, WorkflowValidationError> {
    let value = value.ok_or_else(|| invalid_parameters(node, format!("{label} is required")))?;
    parse_node_ref_value(value, Some(node), label)
}

pub(crate) fn optional_node_ref(
    value: Option<&Value>,
    node: &WorkflowNode,
    label: &str,
) -> Result<Option<WorkflowNodeRef>, WorkflowValidationError> {
    value
        .map(|value| parse_node_ref_value(value, Some(node), label))
        .transpose()
}

pub(crate) fn parse_node_ref_value(
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

pub(crate) fn node_ref_array(
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

pub(crate) fn invalid_parameters(
    node: &WorkflowNode,
    message: impl Into<String>,
) -> WorkflowValidationError {
    WorkflowValidationError::InvalidNodeParameters {
        node: node.id.as_str().to_string(),
        message: message.into(),
    }
}

pub(crate) fn value_refs(
    node: &WorkflowNode,
) -> Result<Vec<WorkflowValueRef>, WorkflowValidationError> {
    let mut refs = Vec::new();
    collect_value_refs(&node.parameters, &mut refs)?;
    collect_value_refs(&node.condition, &mut refs)?;
    for branch in &node.transitions.branches {
        collect_value_refs(&branch.when, &mut refs)?;
    }
    Ok(refs)
}

pub(crate) fn collect_value_refs(
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
                || map.contains_key("$to_string")
                || map.contains_key("$to_json_string")
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
            } else if let Some(nested) = map.get("$to_string") {
                collect_value_refs(nested, refs)?;
            } else if let Some(nested) = map.get("$to_json_string") {
                collect_value_refs(nested, refs)?;
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
