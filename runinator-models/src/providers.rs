use serde::{Deserialize, Serialize};
use serde_json::Value;

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
            let type_str = match param.value_type {
                ParameterValueType::String => "string",
                ParameterValueType::Integer => "integer",
                ParameterValueType::Number => "number",
                ParameterValueType::Boolean => "boolean",
                ParameterValueType::StringArray | ParameterValueType::NumberArray => "array",
                ParameterValueType::Object | ParameterValueType::Json => "object",
            };
            let mut prop = serde_json::json!({ "type": type_str });
            if let Some(desc) = &param.description {
                prop["description"] = serde_json::Value::String(desc.clone());
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
    pub value_type: ParameterValueType,
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
    pub fn required(name: impl Into<String>, value_type: ParameterValueType) -> Self {
        Self {
            name: name.into(),
            value_type,
            label: None,
            description: None,
            required: true,
            default_value: None,
            secret: false,
        }
    }

    pub fn optional(name: impl Into<String>, value_type: ParameterValueType) -> Self {
        Self {
            required: false,
            ..Self::required(name, value_type)
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResultMetadata {
    pub name: String,
    pub value_type: ParameterValueType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl ResultMetadata {
    pub fn new(name: impl Into<String>, value_type: ParameterValueType) -> Self {
        Self {
            name: name.into(),
            value_type,
            schema: None,
            label: None,
            description: None,
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_schema(mut self, schema: Value) -> Self {
        self.schema = Some(schema);
        self
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ParameterValueType {
    String,
    Integer,
    Number,
    Boolean,
    StringArray,
    NumberArray,
    Object,
    Json,
}
