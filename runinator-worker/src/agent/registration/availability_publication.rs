#[allow(unused_imports)]
use super::*;

pub(super) struct AvailabilityPublication<'a> {
    pub(super) availability: &'a AgentAvailability,
    pub(super) reporter: &'a StatusReporter,
    pub(super) report_context: &'a AgentReportContext,
    pub(super) replica_id: Uuid,
    pub(super) runtime_id: &'a str,
    pub(super) heartbeat_seq: u64,
    pub(super) telemetry: Option<&'a TelemetryCollector>,
}
