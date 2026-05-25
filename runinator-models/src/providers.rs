use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use crate::types::RuninatorType;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderMetadata {
    #[serde(alias = "provider_name")]
    pub name: String,
    #[serde(default)]
    pub actions: Vec<ActionMetadata>,
    #[serde(default)]
    pub metadata: ProviderRuntimeMetadata,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderRuntimeMetadata {
    #[serde(default)]
    pub credential_scopes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActionMetadata {
    pub function_name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub parameters: Vec<ParameterMetadata>,
    #[serde(default)]
    pub results: Vec<ResultMetadata>,
}

impl ActionMetadata {
    pub fn new(function_name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            function_name: function_name.into(),
            description: Some(description.into()),
            parameters: Vec::new(),
            results: Vec::new(),
        }
    }

    pub fn with_parameters(mut self, parameters: Vec<ParameterMetadata>) -> Self {
        self.parameters = parameters;
        self
    }

    pub fn with_results(mut self, results: Vec<ResultMetadata>) -> Self {
        self.results = results;
        self
    }

    pub fn to_json_schema(&self) -> serde_json::Value {
        let mut properties = serde_json::Map::new();
        let mut required = Vec::new();
        for param in &self.parameters {
            let mut prop = param.ty.to_json_schema();
            if let Some(desc) = &param.description {
                if let serde_json::Value::Object(object) = &mut prop {
                    object.insert(
                        "description".into(),
                        serde_json::Value::String(desc.clone()),
                    );
                }
            }
            properties.insert(param.name.clone(), prop);
            if param.required {
                required.push(serde_json::Value::String(param.name.clone()));
            }
        }
        serde_json::json!({
            "type": "object",
            "properties": serde_json::Value::Object(properties),
            "required": required,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ParameterMetadata {
    pub name: String,
    #[serde(alias = "value_type", deserialize_with = "deserialize_type")]
    pub ty: RuninatorType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_value: Option<Value>,
    #[serde(default)]
    pub secret: bool,
}

impl ParameterMetadata {
    pub fn required(name: impl Into<String>, ty: RuninatorType) -> Self {
        Self {
            name: name.into(),
            ty,
            label: None,
            description: None,
            required: true,
            default_value: None,
            secret: false,
        }
    }

    pub fn optional(name: impl Into<String>, ty: RuninatorType) -> Self {
        Self {
            required: false,
            ..Self::required(name, ty)
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn secret(mut self) -> Self {
        self.secret = true;
        self
    }

    pub fn with_default(mut self, default_value: Value) -> Self {
        self.default_value = Some(default_value);
        self
    }
}

pub fn validate_provider_metadata(metadata: &ProviderMetadata) -> Result<(), String> {
    if metadata.name.trim().is_empty() {
        return Err("provider name is required".into());
    }
    for action in &metadata.actions {
        validate_action_metadata(metadata, action)?;
    }
    Ok(())
}

fn validate_action_metadata(
    provider: &ProviderMetadata,
    action: &ActionMetadata,
) -> Result<(), String> {
    if action.function_name.trim().is_empty() {
        return Err(format!(
            "provider '{}' has an action without a function name",
            provider.name
        ));
    }
    let mut names = std::collections::BTreeSet::new();
    for parameter in &action.parameters {
        if parameter.name.trim().is_empty() {
            return Err(format!(
                "provider '{}.{}' has a parameter without a name",
                provider.name, action.function_name
            ));
        }
        if !names.insert(parameter.name.as_str()) {
            return Err(format!(
                "provider '{}.{}' has duplicate parameter '{}'",
                provider.name, action.function_name, parameter.name
            ));
        }
        if let Some(default_value) = &parameter.default_value {
            parameter
                .ty
                .validate_value(default_value)
                .map_err(|violation| {
                    violation.message_with_label(&format!(
                        "provider '{}.{}' parameter '{}'",
                        provider.name, action.function_name, parameter.name
                    ))
                })?;
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResultMetadata {
    pub name: String,
    #[serde(alias = "value_type", deserialize_with = "deserialize_type")]
    pub ty: RuninatorType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl ResultMetadata {
    pub fn new(name: impl Into<String>, ty: RuninatorType) -> Self {
        Self {
            name: name.into(),
            ty,
            label: None,
            description: None,
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_type(mut self, ty: RuninatorType) -> Self {
        self.ty = ty;
        self
    }
}

fn deserialize_type<'de, D>(deserializer: D) -> Result<RuninatorType, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    if let Some(raw) = value.as_str() {
        return Ok(match raw {
            "string" => RuninatorType::String,
            "integer" => RuninatorType::Integer,
            "number" => RuninatorType::Number,
            "boolean" => RuninatorType::Boolean,
            "string_array" => RuninatorType::array(RuninatorType::String),
            "number_array" => RuninatorType::array(RuninatorType::Number),
            "object" => RuninatorType::map(RuninatorType::Any),
            "json" => RuninatorType::Any,
            other => {
                return Err(serde::de::Error::custom(format!(
                    "unknown legacy value type '{other}'"
                )));
            }
        });
    }
    serde_json::from_value(value).map_err(serde::de::Error::custom)
}
