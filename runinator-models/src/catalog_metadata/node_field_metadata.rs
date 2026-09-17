#[allow(unused_imports)]
use super::*;

/// a form field bound to a specific location within the node json.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NodeFieldMetadata {
    #[serde(flatten)]
    pub field: UiField,
    pub location: FieldLocation,
}

impl NodeFieldMetadata {
    pub fn new(field: impl Into<UiField>, location: FieldLocation) -> Self {
        Self {
            field: field.into(),
            location,
        }
    }
}
