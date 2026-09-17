#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdapterConfigurationField {
    pub name: String,
    pub value_type: RuninatorType,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub secret: bool,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub default: Value,
}
