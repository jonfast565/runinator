//! Type-guided workflow launch input used by the operations console.

use runinator_models::{types::RuninatorType, value::Value, workflows::WorkflowDefinition};
use uuid::Uuid;

fn collect_fields(
    ty: &RuninatorType,
    path: Vec<String>,
    required: bool,
    default: Option<Value>,
    output: &mut Vec<FormField>,
) {
    let RuninatorType::Struct {
        fields,
        additional: None,
    } = ty
    else {
        output.push(FormField {
            path,
            ty: ty.clone(),
            required,
            default,
        });
        return;
    };
    for (name, field) in fields {
        let mut nested = path.clone();
        nested.push(name.clone());
        collect_fields(
            &field.ty,
            nested,
            required && field.required,
            field.default.clone(),
            output,
        );
    }
}

fn parse_value(text: &str, ty: &RuninatorType) -> Result<serde_json::Value, String> {
    match ty {
        RuninatorType::String | RuninatorType::Duration => Ok(text.into()),
        RuninatorType::Boolean => text
            .parse::<bool>()
            .map(serde_json::Value::Bool)
            .map_err(|_| "enter true or false".into()),
        RuninatorType::Integer => text
            .parse::<i64>()
            .map(Into::into)
            .map_err(|_| "enter an integer".into()),
        RuninatorType::Number => text
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map(serde_json::Value::Number)
            .ok_or_else(|| "enter a finite number".into()),
        RuninatorType::Null => Ok(serde_json::Value::Null),
        RuninatorType::Range { base, .. } => parse_value(text, base),
        RuninatorType::Enum(_) => serde_json::from_str(text).or_else(|_| Ok(text.into())),
        _ => serde_json::from_str(text)
            .map_err(|error| format!("enter valid JSON for {}: {error}", ty.describe())),
    }
}

fn insert_path(
    root: &mut serde_json::Value,
    path: &[String],
    value: serde_json::Value,
) -> Result<(), String> {
    if path.is_empty() {
        *root = value;
        return Ok(());
    }
    let mut current = root;
    for segment in &path[..path.len() - 1] {
        let object = current
            .as_object_mut()
            .ok_or_else(|| format!("{} is not an object", segment))?;
        current = object
            .entry(segment.clone())
            .or_insert_with(|| serde_json::Value::Object(Default::default()));
    }
    let object = current
        .as_object_mut()
        .ok_or_else(|| "workflow input is not an object".to_string())?;
    object.insert(path[path.len() - 1].clone(), value);
    Ok(())
}

#[cfg(test)]
#[path = "form_tests.rs"]
mod tests;

mod launch_form;
pub(super) use launch_form::LaunchForm;

mod form_field;
use form_field::FormField;
