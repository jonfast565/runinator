#[allow(unused_imports)]
use super::*;

#[derive(Debug)]
pub(crate) struct LaunchForm {
    pub workflow_id: Uuid,
    pub workflow_name: String,
    pub(super) input_type: RuninatorType,
    pub(super) fields: Vec<FormField>,
    pub(super) values: serde_json::Value,
    pub run_name: Option<String>,
    pub index: usize,
    pub buffer: String,
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
