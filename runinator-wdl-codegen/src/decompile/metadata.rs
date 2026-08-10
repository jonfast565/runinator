use std::collections::{HashMap, HashSet};

use runinator_models::{types::RuninatorType, value::Value};

use super::expr;

pub(super) fn read_declared_types(metadata: &Value) -> HashMap<String, String> {
    let mut types = HashMap::new();
    let Some(entries) = metadata.pointer("/wdl/types").and_then(Value::as_object) else {
        return types;
    };
    for (id, value) in entries {
        if let Some(text) = value.as_str() {
            types.insert(id.clone(), text.to_string());
        } else if let Ok(ty) = value.decode::<RuninatorType>() {
            types.insert(id.clone(), expr::render_type(&ty));
        }
    }
    types
}

pub(super) fn read_type_decls(metadata: &Value) -> Vec<(String, String)> {
    let Some(entries) = metadata
        .pointer("/wdl/type_decls")
        .and_then(Value::as_object)
    else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|(name, value)| {
            value
                .as_str()
                .map(|text| (name.clone(), text.to_string()))
                .or_else(|| {
                    value
                        .decode::<RuninatorType>()
                        .ok()
                        .map(|ty| (name.clone(), expr::render_type(&ty)))
                })
        })
        .collect()
}

pub(super) fn read_output_type(metadata: &Value) -> Option<RuninatorType> {
    metadata.pointer("/wdl/output_type")?.decode().ok()
}

pub(super) fn read_input_types(metadata: &Value) -> HashMap<String, String> {
    let mut overrides = HashMap::new();
    let Some(entries) = metadata
        .pointer("/wdl/input_types")
        .and_then(Value::as_object)
    else {
        return overrides;
    };
    for (name, value) in entries {
        if let Some(text) = value.as_str() {
            overrides.insert(name.clone(), text.to_string());
        }
    }
    overrides
}

fn read_array(metadata: &Value, pointer: &str) -> Vec<Value> {
    metadata
        .pointer(pointer)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

pub(super) fn read_triggers(metadata: &Value) -> Vec<Value> {
    read_array(metadata, "/triggers")
}

pub(super) fn read_notifications(metadata: &Value) -> Vec<Value> {
    read_array(metadata, "/notifications")
}

pub(super) fn read_interrupts(metadata: &Value) -> Vec<Value> {
    read_array(metadata, "/interrupts")
}

pub(super) fn read_watches(metadata: &Value) -> Vec<Value> {
    read_array(metadata, "/watches")
}

pub(super) fn read_alias_decls(metadata: &Value) -> Vec<(String, Vec<Value>)> {
    let Some(entries) = metadata.pointer("/wdl/aliases").and_then(Value::as_array) else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| {
            Some((
                entry.get("name").and_then(Value::as_str)?.to_string(),
                entry.get("segs").and_then(Value::as_array)?.clone(),
            ))
        })
        .collect()
}

pub(super) fn read_spreads(metadata: &Value) -> HashMap<String, Vec<Value>> {
    let mut spreads = HashMap::new();
    let Some(entries) = metadata.pointer("/wdl/spreads").and_then(Value::as_object) else {
        return spreads;
    };
    for (id, segments) in entries {
        if let Some(segments) = segments.as_array() {
            spreads.insert(id.clone(), segments.clone());
        }
    }
    spreads
}

pub(super) fn read_control_ids(metadata: &Value) -> HashSet<String> {
    metadata
        .pointer("/wdl/control_ids")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}
