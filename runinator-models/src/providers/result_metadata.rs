#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResultMetadata {
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
