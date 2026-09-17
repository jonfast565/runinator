#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone)]
pub struct NewOrchestrationCommand {
    pub id: Uuid,
    pub binding_id: Uuid,
    pub epoch: i64,
    pub command_type: String,
    pub operation_key: String,
    pub payload: Value,
}
