#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectState {
    pub name: String,
    pub items: Vec<Value>,
    pub threshold: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deadline_unix: Option<i64>,
}
