#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaceOutput {
    pub winner: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<Value>,
}
