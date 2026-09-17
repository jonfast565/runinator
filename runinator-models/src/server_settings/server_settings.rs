#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[derive(Default)]
pub struct ServerSettings {
    pub authentication: AuthenticationSettings,
    pub orchestration: OrchestrationSettings,
    pub notifications: NotificationSettings,
    pub workers: WorkerSettings,
    pub wakers: WakerSettings,
    pub background_engine: BackgroundEngineSettings,
    pub replicas: ReplicaSettings,
    pub archiver: ArchiverSettings,
}

impl ServerSettings {
    pub fn validate(&self) -> Result<(), String> {
        for definition in server_setting_catalog() {
            if definition.kind == ServerSettingKind::Boolean {
                if self.boolean_value(definition.key).is_none() {
                    return Err(format!(
                        "{} is missing from the server settings model",
                        definition.key
                    ));
                }
                continue;
            }
            let value = self
                .integer_value(definition.key)
                .expect("catalog key must resolve");
            if !(definition.minimum..=definition.maximum).contains(&value) {
                return Err(format!(
                    "{} must be between {} and {} {}",
                    definition.key, definition.minimum, definition.maximum, definition.unit
                ));
            }
        }
        if self.replicas.reap_after_seconds <= self.replicas.stale_after_seconds {
            return Err(
                "replicas.reap_after_seconds must be greater than replicas.stale_after_seconds"
                    .into(),
            );
        }
        if self.replicas.delete_after_seconds <= self.replicas.reap_after_seconds {
            return Err(
                "replicas.delete_after_seconds must be greater than replicas.reap_after_seconds"
                    .into(),
            );
        }
        Ok(())
    }

    pub(super) fn integer_value(&self, key: &str) -> Option<u64> {
        Some(match key {
            "authentication.max_refreshes" => self.authentication.max_refreshes,
            "orchestration.claim_batch_size" => self.orchestration.claim_batch_size,
            "orchestration.trigger_poll_interval_ms" => self.orchestration.trigger_poll_interval_ms,
            "orchestration.agent_directive_poll_interval_ms" => {
                self.orchestration.agent_directive_poll_interval_ms
            }
            "orchestration.workflow_vm_poll_interval_ms" => {
                self.orchestration.workflow_vm_poll_interval_ms
            }
            "orchestration.effect_dispatch_poll_interval_ms" => {
                self.orchestration.effect_dispatch_poll_interval_ms
            }
            "orchestration.correlated_reducer_poll_interval_ms" => {
                self.orchestration.correlated_reducer_poll_interval_ms
            }
            "orchestration.correlated_reducer_lease_seconds" => {
                self.orchestration.correlated_reducer_lease_seconds
            }
            "orchestration.action_dispatch_lease_seconds" => {
                self.orchestration.action_dispatch_lease_seconds
            }
            "orchestration.action_deadline_grace_seconds" => {
                self.orchestration.action_deadline_grace_seconds
            }
            "orchestration.timer_arm_horizon_ms" => self.orchestration.timer_arm_horizon_ms,
            "orchestration.workspace_reconcile_interval_seconds" => {
                self.orchestration.workspace_reconcile_interval_seconds
            }
            "orchestration.usage_sample_interval_seconds" => {
                self.orchestration.usage_sample_interval_seconds
            }
            "orchestration.operational_metrics_interval_seconds" => {
                self.orchestration.operational_metrics_interval_seconds
            }
            "orchestration.adapter_diagnostic_retention_seconds" => {
                self.orchestration.adapter_diagnostic_retention_seconds
            }
            "orchestration.settings_refresh_interval_seconds" => {
                self.orchestration.settings_refresh_interval_seconds
            }
            "orchestration.synchronous_invocation_wait_ms" => {
                self.orchestration.synchronous_invocation_wait_ms
            }
            "orchestration.synchronous_invocation_poll_ms" => {
                self.orchestration.synchronous_invocation_poll_ms
            }
            "notifications.scan_interval_seconds" => self.notifications.scan_interval_seconds,
            "notifications.scan_limit" => self.notifications.scan_limit,
            "notifications.secret_expiry_warning_seconds" => {
                self.notifications.secret_expiry_warning_seconds
            }
            "notifications.delivery_timeout_seconds" => self.notifications.delivery_timeout_seconds,
            "workers.max_concurrent_actions" => self.workers.max_concurrent_actions,
            "workers.shutdown_grace_seconds" => self.workers.shutdown_grace_seconds,
            "workers.reconnect_max_attempts" => self.workers.reconnect_max_attempts,
            "workers.settings_refresh_interval_seconds" => {
                self.workers.settings_refresh_interval_seconds
            }
            "wakers.max_concurrent_wakes" => self.wakers.max_concurrent_wakes,
            "wakers.max_wake_sleep_seconds" => self.wakers.max_wake_sleep_seconds,
            "background_engine.max_concurrent_ingress" => {
                self.background_engine.max_concurrent_ingress
            }
            "replicas.stale_after_seconds" => self.replicas.stale_after_seconds,
            "replicas.reap_after_seconds" => self.replicas.reap_after_seconds,
            "replicas.delete_after_seconds" => self.replicas.delete_after_seconds,
            "replicas.reaper_interval_seconds" => self.replicas.reaper_interval_seconds,
            "replicas.sample_retention_seconds" => self.replicas.sample_retention_seconds,
            "replicas.sample_window_seconds" => self.replicas.sample_window_seconds,
            "replicas.sample_max_points" => self.replicas.sample_max_points,
            "archiver.interval_seconds" => self.archiver.interval_seconds,
            "archiver.claim_lease_seconds" => self.archiver.claim_lease_seconds,
            "archiver.batch_size" => self.archiver.batch_size,
            "archiver.workflow_run_retention_seconds" => {
                self.archiver.workflow_run_retention_seconds
            }
            "archiver.pipeline_run_retention_seconds" => {
                self.archiver.pipeline_run_retention_seconds
            }
            "archiver.orchestration_retention_seconds" => {
                self.archiver.orchestration_retention_seconds
            }
            "archiver.effect_dispatch_retention_seconds" => {
                self.archiver.effect_dispatch_retention_seconds
            }
            "archiver.notification_retention_seconds" => {
                self.archiver.notification_retention_seconds
            }
            "archiver.dead_letter_retention_seconds" => self.archiver.dead_letter_retention_seconds,
            "archiver.audit_log_retention_seconds" => self.archiver.audit_log_retention_seconds,
            "archiver.idempotency_retention_seconds" => self.archiver.idempotency_retention_seconds,
            "archiver.automation_retention_seconds" => self.archiver.automation_retention_seconds,
            "archiver.usage_retention_seconds" => self.archiver.usage_retention_seconds,
            "archiver.revision_retention_seconds" => self.archiver.revision_retention_seconds,
            "archiver.agent_directive_retention_seconds" => {
                self.archiver.agent_directive_retention_seconds
            }
            "archiver.archive_ledger_retention_seconds" => {
                self.archiver.archive_ledger_retention_seconds
            }
            "archiver.security_retention_seconds" => self.archiver.security_retention_seconds,
            "archiver.coordination_retention_seconds" => {
                self.archiver.coordination_retention_seconds
            }
            _ => return None,
        })
    }

    pub(super) fn boolean_value(&self, key: &str) -> Option<bool> {
        match key {
            "archiver.dry_run" => Some(self.archiver.dry_run),
            _ => None,
        }
    }
}

impl Validate for ServerSettings {
    fn validate(&self) -> Result<(), ValidationError> {
        ServerSettings::validate(self).map_err(|message| ValidationError::new("settings", message))
    }
}
