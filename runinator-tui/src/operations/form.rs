//! Type-guided workflow launch input used by the operations console.

use runinator_models::{types::RuninatorType, value::Value, workflows::WorkflowDefinition};
use uuid::Uuid;

#[derive(Debug)]
pub(super) struct LaunchForm {
    pub workflow_id: Uuid,
    pub workflow_name: String,
    input_type: RuninatorType,
    fields: Vec<FormField>,
    values: serde_json::Value,
    pub run_name: Option<String>,
    pub index: usize,
    pub buffer: String,
}

#[derive(Debug)]
struct FormField {
    path: Vec<String>,
    ty: RuninatorType,
    required: bool,
    default: Option<Value>,
}

impl LaunchForm {
    pub fn new(workflow: &WorkflowDefinition) -> Result<Self, String> {
        let workflow_id = workflow
            .id
            .ok_or_else(|| "selected workflow has no id".to_string())?;
        let mut fields = Vec::new();
        collect_fields(&workflow.input_type, Vec::new(), true, None, &mut fields);
        let values = if matches!(workflow.input_type, RuninatorType::Struct { .. }) {
            serde_json::Value::Object(Default::default())
        } else {
            serde_json::Value::Null
        };
        Ok(Self {
            workflow_id,
            workflow_name: workflow.name.clone(),
            input_type: workflow.input_type.clone(),
            fields,
            values,
            run_name: None,
            index: 0,
            buffer: String::new(),
        })
    }

    pub fn prompt(&self) -> String {
        if self.index == 0 {
            return format!(
                "{} · run name (optional): {}_",
                self.workflow_name, self.buffer
            );
        }
        let field = &self.fields[self.index - 1];
        let path = if field.path.is_empty() {
            "input".to_string()
        } else {
            field.path.join(".")
        };
        let requirement = if field.required {
            "required"
        } else {
            "optional"
        };
        let default = field
            .default
            .as_ref()
            .map(|value| format!(" · default {value}"))
            .unwrap_or_default();
        format!(
            "{} · {path} ({}, {requirement}{default}): {}_",
            self.workflow_name,
            field.ty.describe(),
            self.buffer
        )
    }

    /// Accept the current entry. Returns true when the form is complete.
    pub fn advance(&mut self) -> Result<bool, String> {
        if self.index == 0 {
            let name = self.buffer.trim();
            self.run_name = (!name.is_empty()).then(|| name.to_string());
            self.buffer.clear();
            self.index += 1;
            return Ok(self.fields.is_empty());
        }
        let field = &self.fields[self.index - 1];
        let text = self.buffer.trim();
        if text.is_empty() {
            if field.required && field.default.is_none() {
                return Err(format!("{} is required", field.path.join(".")));
            }
        } else {
            let value = parse_value(text, &field.ty)?;
            insert_path(&mut self.values, &field.path, value)?;
        }
        self.buffer.clear();
        self.index += 1;
        Ok(self.index > self.fields.len())
    }

    pub fn finish(self) -> Result<(Uuid, Value, Option<String>), String> {
        let value = Value::from(self.values);
        self.input_type
            .validate_value(&value)
            .map_err(|error| error.message_with_label("workflow input"))?;
        Ok((self.workflow_id, value, self.run_name))
    }
}

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
