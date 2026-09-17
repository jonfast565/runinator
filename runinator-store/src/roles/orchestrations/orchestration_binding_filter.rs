#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Default)]
pub struct OrchestrationBindingFilter {
    pub status: Option<OrchestrationStatus>,
    pub pipeline_id: Option<Uuid>,
    pub adapter_id: Option<Uuid>,
    pub scope: Option<String>,
    pub scope_prefix: Option<String>,
    pub correlation_key: Option<String>,
    pub limit: i64,
}
