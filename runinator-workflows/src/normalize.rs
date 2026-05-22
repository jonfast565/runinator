use std::collections::HashSet;

use runinator_models::workflows::WorkflowDefinition;
use serde_json::{Map, Value};

use crate::parameters::parse_node_ref_value;

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
    ensure_fail_node(&mut nodes, &mut ids);
    let previous_start = existing_start
        .filter(|id| ids.contains(id) && node_kind_by_id(&nodes, id).as_deref() != Some("start"))
        .or_else(|| {
            first_node_id(&nodes, |kind| {
                kind != Some("start") && kind != Some("end") && kind != Some("fail")
            })
        })
        .unwrap_or_else(|| end_id.clone());
    let start_id = ensure_start_node(&mut nodes, &mut ids, &previous_start, &end_id);

    route_success_terminals_to_end(&mut nodes, &end_id);
    root.insert("start".into(), Value::String(start_id));
    root.insert("nodes".into(), Value::Array(nodes));
    Value::Object(root)
}

pub fn normalize_layout(root: &mut Map<String, Value>) {
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

pub(crate) fn node_ids(nodes: &[Value]) -> HashSet<String> {
    nodes
        .iter()
        .filter_map(|node| node.get("id").and_then(Value::as_str).map(str::to_string))
        .collect()
}

pub(crate) fn ensure_end_node(nodes: &mut Vec<Value>, ids: &mut HashSet<String>) -> String {
    if let Some(id) = first_node_id(nodes, |kind| kind == Some("end")) {
        return id;
    }
    let id = unique_node_id("end", ids);
    nodes.push(serde_json::json!({ "id": id, "kind": "end" }));
    id
}

pub(crate) fn ensure_fail_node(nodes: &mut Vec<Value>, ids: &mut HashSet<String>) -> String {
    if let Some(id) = first_node_id(nodes, |kind| kind == Some("fail")) {
        return id;
    }
    let id = unique_node_id("fail", ids);
    nodes.push(serde_json::json!({ "id": id, "kind": "fail" }));
    id
}

pub(crate) fn ensure_start_node(
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

pub(crate) fn route_success_terminals_to_end(nodes: &mut [Value], end_id: &str) {
    for node in nodes {
        if matches!(node_kind(node).as_deref(), Some("end") | Some("fail")) {
            continue;
        }
        if has_success_transition(node) {
            continue;
        }
        ensure_next_transition(node, end_id);
    }
}

pub(crate) fn ensure_next_transition(node: &mut Value, target: &str) {
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

pub(crate) fn has_success_transition(node: &Value) -> bool {
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

pub(crate) fn valid_node_ref_value(value: Option<&Value>) -> bool {
    value.is_some_and(|value| parse_node_ref_value(value, None, "transition").is_ok())
}

pub(crate) fn first_node_id(
    nodes: &[Value],
    predicate: impl Fn(Option<&str>) -> bool,
) -> Option<String> {
    nodes.iter().find_map(|node| {
        if predicate(node_kind(node).as_deref()) {
            return node_id(node);
        }
        None
    })
}

pub(crate) fn node_id(node: &Value) -> Option<String> {
    node.get("id").and_then(Value::as_str).map(str::to_string)
}

pub(crate) fn node_kind(node: &Value) -> Option<String> {
    node.get("kind").and_then(Value::as_str).map(str::to_string)
}

pub(crate) fn node_kind_by_id(nodes: &[Value], id: &str) -> Option<String> {
    nodes
        .iter()
        .find(|node| node_id(node).as_deref() == Some(id))
        .and_then(node_kind)
}

pub(crate) fn unique_node_id(base: &str, ids: &mut HashSet<String>) -> String {
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
