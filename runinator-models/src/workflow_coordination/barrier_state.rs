#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BarrierState {
    pub name: String,
    pub expected_count: i64,
    pub arrivals: Vec<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deadline_unix: Option<i64>,
}
