#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Copy, Default)]
pub struct OrchestrationListQuery<'a> {
    pub status: Option<&'a str>,
    pub pipeline_id: Option<Uuid>,
    pub adapter_id: Option<Uuid>,
    pub scope: Option<&'a str>,
    pub correlation_key: Option<&'a str>,
    pub limit: Option<i64>,
    pub scope_prefix: Option<&'a str>,
}
