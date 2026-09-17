#[allow(unused_imports)]
use super::*;

/// Hot-reloadable retention and sweep policy used by every archiver replica.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ArchiverSettings {
    pub interval_seconds: u64,
    pub claim_lease_seconds: u64,
    pub batch_size: u64,
    pub dry_run: bool,
    pub workflow_run_retention_seconds: u64,
    pub pipeline_run_retention_seconds: u64,
    pub orchestration_retention_seconds: u64,
    pub effect_dispatch_retention_seconds: u64,
    pub notification_retention_seconds: u64,
    pub dead_letter_retention_seconds: u64,
    pub audit_log_retention_seconds: u64,
    pub idempotency_retention_seconds: u64,
    pub automation_retention_seconds: u64,
    pub usage_retention_seconds: u64,
    pub revision_retention_seconds: u64,
    pub agent_directive_retention_seconds: u64,
    pub archive_ledger_retention_seconds: u64,
    pub security_retention_seconds: u64,
    pub coordination_retention_seconds: u64,
}

impl Default for ArchiverSettings {
    fn default() -> Self {
        Self {
            interval_seconds: 3_600,
            claim_lease_seconds: 600,
            batch_size: 1_000,
            dry_run: false,
            workflow_run_retention_seconds: 7_776_000,
            pipeline_run_retention_seconds: 7_776_000,
            orchestration_retention_seconds: 7_776_000,
            effect_dispatch_retention_seconds: 604_800,
            notification_retention_seconds: 2_592_000,
            dead_letter_retention_seconds: 7_776_000,
            audit_log_retention_seconds: 31_536_000,
            idempotency_retention_seconds: 604_800,
            automation_retention_seconds: 7_776_000,
            usage_retention_seconds: 31_536_000,
            revision_retention_seconds: 31_536_000,
            agent_directive_retention_seconds: 2_592_000,
            archive_ledger_retention_seconds: 2_592_000,
            security_retention_seconds: 604_800,
            coordination_retention_seconds: 2_592_000,
        }
    }
}
