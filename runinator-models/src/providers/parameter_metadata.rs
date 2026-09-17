#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ParameterMetadata {
    pub name: String,
    // `type` is accepted as well as `ty`: these schemas are hand-written in function
    // manifests, and `ty` is a rust field name rather than something an author would reach for.
    // serialization still emits `ty`, so nothing downstream sees a second spelling.
    #[serde(alias = "type", deserialize_with = "deserialize_type")]
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
    /// Worker-side destinations populated from this secret parameter after late resolution.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub credential_injections: Vec<CredentialInjection>,
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
            credential_injections: Vec::new(),
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

    pub fn inject(mut self, injection: CredentialInjection) -> Self {
        self.credential_injections.push(injection);
        self
    }

    pub fn with_default(mut self, default_value: impl Into<Value>) -> Self {
        self.default_value = Some(default_value.into());
        self
    }
}
