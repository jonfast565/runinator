#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct OrchestrationSettings {
    pub claim_batch_size: u64,
    pub trigger_poll_interval_ms: u64,
    pub agent_directive_poll_interval_ms: u64,
    pub workflow_vm_poll_interval_ms: u64,
    pub effect_dispatch_poll_interval_ms: u64,
    pub correlated_reducer_poll_interval_ms: u64,
    pub correlated_reducer_lease_seconds: u64,
    pub action_dispatch_lease_seconds: u64,
    pub action_deadline_grace_seconds: u64,
    pub timer_arm_horizon_ms: u64,
    pub workspace_reconcile_interval_seconds: u64,
    pub usage_sample_interval_seconds: u64,
    pub operational_metrics_interval_seconds: u64,
    pub adapter_diagnostic_retention_seconds: u64,
    pub settings_refresh_interval_seconds: u64,
    pub synchronous_invocation_wait_ms: u64,
    pub synchronous_invocation_poll_ms: u64,
}

impl Default for OrchestrationSettings {
    fn default() -> Self {
        Self {
            claim_batch_size: 100,
            trigger_poll_interval_ms: 1_000,
            agent_directive_poll_interval_ms: 1_000,
            workflow_vm_poll_interval_ms: 250,
            effect_dispatch_poll_interval_ms: 250,
            correlated_reducer_poll_interval_ms: 250,
            correlated_reducer_lease_seconds: 60,
            action_dispatch_lease_seconds: 60,
            action_deadline_grace_seconds: 30,
            timer_arm_horizon_ms: 1_000,
            workspace_reconcile_interval_seconds: 60,
            usage_sample_interval_seconds: 300,
            operational_metrics_interval_seconds: 15,
            adapter_diagnostic_retention_seconds: 7 * 24 * 60 * 60,
            settings_refresh_interval_seconds: 5,
            synchronous_invocation_wait_ms: 5_000,
            synchronous_invocation_poll_ms: 200,
        }
    }
}
