#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct StabilityCounters {
    pub result_events_applied: u64,
    pub result_events_duplicate: u64,
    pub result_events_retried: u64,
    pub result_events_dead_lettered: u64,
    pub result_receive_errors: u64,
}
