#[allow(unused_imports)]
use super::*;

pub(super) struct BrokerTraceCorrelation {
    pub(super) workflow_run_id: Option<Uuid>,
    pub(super) delivery_id: Option<Uuid>,
    pub(super) dedupe_key: Option<String>,
    pub(super) trace_id: Option<Uuid>,
}
