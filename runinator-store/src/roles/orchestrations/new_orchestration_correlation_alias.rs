#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone)]
pub struct NewOrchestrationCorrelationAlias {
    pub id: Uuid,
    pub binding_id: Uuid,
    pub generation: i64,
    pub org_id: Option<Uuid>,
    pub source: String,
    pub scope: String,
    pub correlation_key: String,
}
