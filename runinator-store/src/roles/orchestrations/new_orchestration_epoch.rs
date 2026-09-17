#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone)]
pub struct NewOrchestrationEpoch {
    pub id: Uuid,
    pub binding_id: Uuid,
    pub epoch: i64,
    pub start_member: Option<String>,
    pub parameters: Value,
    pub reason: String,
}
