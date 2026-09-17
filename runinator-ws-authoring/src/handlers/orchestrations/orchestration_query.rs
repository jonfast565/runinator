#[allow(unused_imports)]
use super::*;

#[derive(Debug, Default, Deserialize)]
pub struct OrchestrationQuery {
    pub status: Option<String>,
    pub pipeline_id: Option<Uuid>,
    pub adapter_id: Option<Uuid>,
    pub scope: Option<String>,
    pub scope_prefix: Option<String>,
    pub correlation_key: Option<String>,
    pub limit: Option<i64>,
}
