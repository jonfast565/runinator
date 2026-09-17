#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BarrierOutput {
    pub name: String,
    pub arrivals: Vec<Uuid>,
}
