use runinator_models::workflows::WorkflowNodeRef;
use serde_json::{Map, Value};

use crate::errors::WorkflowValidationError;
use crate::types::{WorkflowExpression, WorkflowPathSegment, WorkflowRefSource, WorkflowValueRef};

pub fn resolve_value_refs(
    value: &Value,
    context: &Value,
) -> Result<Value, WorkflowValidationError> {
    let expression = parse_expression(value)?;
    evaluate_expression(&expression, context)
}

pub(crate) fn parse_expression(
    value: &Value,
) -> Result<WorkflowExpression, WorkflowValidationError> {
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

pub(crate) fn evaluate_static_expression(
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

pub(crate) fn evaluate_expression(
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

pub(crate) fn parse_value_ref(value: &Value) -> Result<WorkflowValueRef, WorkflowValidationError> {
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

pub(crate) fn parse_path(
    value: &Value,
) -> Result<Vec<WorkflowPathSegment>, WorkflowValidationError> {
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

pub(crate) fn resolve_value_ref(
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

pub(crate) fn resolve_path<'a>(
    value: &'a Value,
    path: &[WorkflowPathSegment],
) -> Option<&'a Value> {
    let mut current = value;
    for segment in path {
        current = match segment {
            WorkflowPathSegment::Key(key) => current.get(key)?,
            WorkflowPathSegment::Index(index) => current.get(*index)?,
        };
    }
    Some(current)
}

pub(crate) fn serialize_value_ref(reference: &WorkflowValueRef) -> Value {
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

pub(crate) fn template_value_to_string(value: Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(value) => value,
        other => other.to_string(),
    }
}
