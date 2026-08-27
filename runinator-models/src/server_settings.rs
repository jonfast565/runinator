//! Typed, platform-wide operating policy.
//!
//! These values are deliberately separate from process bootstrap configuration (addresses,
//! credentials, backend selection, and command-line defaults) and from protocol/safety constants.
//! The catalog is returned with the current values so every administrative client renders the
//! same bounds, defaults, units, and operator guidance that the server validates.

use serde::{Deserialize, Serialize};

use crate::settings::SettingKind;

pub const SERVER_SETTINGS_SCOPE: &str = "server";
pub const SERVER_SETTINGS_NAME: &str = "operational_policy";

pub fn is_reserved_server_setting(kind: SettingKind, scope: &str, name: &str) -> bool {
    kind == SettingKind::Config && scope == SERVER_SETTINGS_SCOPE && name == SERVER_SETTINGS_NAME
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ServerSettings {
    pub authentication: AuthenticationSettings,
    pub orchestration: OrchestrationSettings,
    pub notifications: NotificationSettings,
    pub replicas: ReplicaSettings,
}

impl Default for ServerSettings {
    fn default() -> Self {
        Self {
            authentication: AuthenticationSettings::default(),
            orchestration: OrchestrationSettings::default(),
            notifications: NotificationSettings::default(),
            replicas: ReplicaSettings::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AuthenticationSettings {
    pub max_refreshes: u64,
}

impl Default for AuthenticationSettings {
    fn default() -> Self {
        Self { max_refreshes: 100 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct OrchestrationSettings {
    pub claim_batch_size: u64,
    pub trigger_poll_interval_ms: u64,
    pub agent_directive_poll_interval_ms: u64,
    pub workflow_vm_poll_interval_ms: u64,
    pub effect_dispatch_poll_interval_ms: u64,
    pub action_dispatch_lease_seconds: u64,
    pub action_deadline_grace_seconds: u64,
    pub timer_arm_horizon_ms: u64,
    pub workspace_reconcile_interval_seconds: u64,
    pub usage_sample_interval_seconds: u64,
    pub operational_metrics_interval_seconds: u64,
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
            action_dispatch_lease_seconds: 60,
            action_deadline_grace_seconds: 30,
            timer_arm_horizon_ms: 1_000,
            workspace_reconcile_interval_seconds: 60,
            usage_sample_interval_seconds: 300,
            operational_metrics_interval_seconds: 15,
            settings_refresh_interval_seconds: 5,
            synchronous_invocation_wait_ms: 5_000,
            synchronous_invocation_poll_ms: 200,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NotificationSettings {
    pub scan_interval_seconds: u64,
    pub scan_limit: u64,
    pub secret_expiry_warning_seconds: u64,
    pub delivery_timeout_seconds: u64,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            scan_interval_seconds: 60,
            scan_limit: 500,
            secret_expiry_warning_seconds: 30 * 24 * 60 * 60,
            delivery_timeout_seconds: 30,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ReplicaSettings {
    pub stale_after_seconds: u64,
    pub reap_after_seconds: u64,
    pub delete_after_seconds: u64,
    pub reaper_interval_seconds: u64,
    pub sample_retention_seconds: u64,
    pub sample_window_seconds: u64,
    pub sample_max_points: u64,
}

impl Default for ReplicaSettings {
    fn default() -> Self {
        Self {
            stale_after_seconds: 30,
            reap_after_seconds: 600,
            delete_after_seconds: 3_600,
            reaper_interval_seconds: 60,
            sample_retention_seconds: 86_400,
            sample_window_seconds: 3_600,
            sample_max_points: 1_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ServerSettingDefinition {
    pub key: &'static str,
    pub section: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub unit: &'static str,
    pub default: u64,
    pub minimum: u64,
    pub maximum: u64,
    pub usual_minimum: u64,
    pub usual_maximum: u64,
}

macro_rules! setting {
    ($key:literal, $section:literal, $label:literal, $description:literal, $unit:literal,
     $default:expr, $min:expr, $max:expr, $usual_min:expr, $usual_max:expr) => {
        ServerSettingDefinition {
            key: $key,
            section: $section,
            label: $label,
            description: $description,
            unit: $unit,
            default: $default,
            minimum: $min,
            maximum: $max,
            usual_minimum: $usual_min,
            usual_maximum: $usual_max,
        }
    };
}

/// The authoritative metadata for every persisted server setting.
pub fn server_setting_catalog() -> Vec<ServerSettingDefinition> {
    vec![
        setting!(
            "authentication.max_refreshes",
            "Authentication",
            "Maximum refreshes",
            "Maximum rotations allowed for one login session.",
            "refreshes",
            100,
            1,
            100_000,
            10,
            1_000
        ),
        setting!(
            "orchestration.claim_batch_size",
            "Orchestration",
            "Claim batch size",
            "Maximum durable work items claimed by one loop iteration.",
            "items",
            100,
            1,
            5_000,
            25,
            500
        ),
        setting!(
            "orchestration.trigger_poll_interval_ms",
            "Orchestration",
            "Trigger poll interval",
            "Delay between scans for due workflow and pipeline triggers.",
            "ms",
            1_000,
            100,
            60_000,
            250,
            5_000
        ),
        setting!(
            "orchestration.agent_directive_poll_interval_ms",
            "Orchestration",
            "Agent directive poll interval",
            "Backstop delay between agent-directive outbox scans.",
            "ms",
            1_000,
            100,
            60_000,
            250,
            5_000
        ),
        setting!(
            "orchestration.workflow_vm_poll_interval_ms",
            "Orchestration",
            "Workflow VM poll interval",
            "Delay used when the continuation driver and timer scheduler have no nudge.",
            "ms",
            250,
            10,
            10_000,
            50,
            1_000
        ),
        setting!(
            "orchestration.effect_dispatch_poll_interval_ms",
            "Orchestration",
            "Effect dispatch poll interval",
            "Delay between workflow and notification effect outbox scans.",
            "ms",
            250,
            10,
            10_000,
            50,
            1_000
        ),
        setting!(
            "orchestration.action_dispatch_lease_seconds",
            "Orchestration",
            "Dispatch lease",
            "Lease held while one engine replica publishes a claimed effect.",
            "seconds",
            60,
            5,
            3_600,
            30,
            300
        ),
        setting!(
            "orchestration.action_deadline_grace_seconds",
            "Orchestration",
            "Action deadline grace",
            "Extra time after the worker timeout before the engine's durable backstop fires.",
            "seconds",
            30,
            1,
            3_600,
            10,
            120
        ),
        setting!(
            "orchestration.timer_arm_horizon_ms",
            "Orchestration",
            "Timer arm horizon",
            "How far ahead the engine publishes workflow timer interrupts to the waker.",
            "ms",
            1_000,
            100,
            60_000,
            500,
            5_000
        ),
        setting!(
            "orchestration.workspace_reconcile_interval_seconds",
            "Orchestration",
            "Workspace reconcile interval",
            "Delay between expired workspace lease reconciliation passes.",
            "seconds",
            60,
            5,
            3_600,
            30,
            300
        ),
        setting!(
            "orchestration.usage_sample_interval_seconds",
            "Orchestration",
            "Usage sample interval",
            "Resolution of resource-allocation usage samples.",
            "seconds",
            300,
            30,
            86_400,
            60,
            900
        ),
        setting!(
            "orchestration.operational_metrics_interval_seconds",
            "Orchestration",
            "Metrics sample interval",
            "Delay between durable queue and fleet metric snapshots.",
            "seconds",
            15,
            1,
            3_600,
            5,
            60
        ),
        setting!(
            "orchestration.settings_refresh_interval_seconds",
            "Orchestration",
            "Settings refresh interval",
            "Maximum delay before engine replicas observe an updated server policy.",
            "seconds",
            5,
            1,
            300,
            2,
            30
        ),
        setting!(
            "orchestration.synchronous_invocation_wait_ms",
            "Orchestration",
            "Synchronous invocation wait",
            "Maximum HTTP wait before a function invocation falls back to an asynchronous response.",
            "ms",
            5_000,
            100,
            120_000,
            1_000,
            15_000
        ),
        setting!(
            "orchestration.synchronous_invocation_poll_ms",
            "Orchestration",
            "Synchronous invocation poll",
            "Delay between run-state checks while an HTTP function invocation waits.",
            "ms",
            200,
            10,
            5_000,
            50,
            500
        ),
        setting!(
            "notifications.scan_interval_seconds",
            "Notifications",
            "Policy scan interval",
            "Delay between scans for duration and secret-expiry notification policies.",
            "seconds",
            60,
            5,
            3_600,
            30,
            300
        ),
        setting!(
            "notifications.scan_limit",
            "Notifications",
            "Policy scan limit",
            "Maximum matching runs inspected in one notification scan.",
            "runs",
            500,
            10,
            10_000,
            100,
            1_000
        ),
        setting!(
            "notifications.secret_expiry_warning_seconds",
            "Notifications",
            "Default secret warning window",
            "Warning window used when a secret-expiry policy omits a threshold.",
            "seconds",
            2_592_000,
            3_600,
            31_536_000,
            604_800,
            7_776_000
        ),
        setting!(
            "notifications.delivery_timeout_seconds",
            "Notifications",
            "Delivery timeout",
            "Worker execution budget for an external notification delivery.",
            "seconds",
            30,
            1,
            3_600,
            10,
            120
        ),
        setting!(
            "replicas.stale_after_seconds",
            "Replicas",
            "Stale after",
            "Default heartbeat silence before a replica is shown as stale.",
            "seconds",
            30,
            5,
            3_600,
            15,
            120
        ),
        setting!(
            "replicas.reap_after_seconds",
            "Replicas",
            "Reap after",
            "Heartbeat silence before a replica is durably marked offline.",
            "seconds",
            600,
            30,
            86_400,
            300,
            3_600
        ),
        setting!(
            "replicas.delete_after_seconds",
            "Replicas",
            "Delete after",
            "Offline retention before a replica row is purged.",
            "seconds",
            3_600,
            60,
            2_592_000,
            1_800,
            86_400
        ),
        setting!(
            "replicas.reaper_interval_seconds",
            "Replicas",
            "Reaper interval",
            "Delay between replica cleanup and telemetry-pruning passes.",
            "seconds",
            60,
            5,
            3_600,
            30,
            300
        ),
        setting!(
            "replicas.sample_retention_seconds",
            "Replicas",
            "Telemetry retention",
            "How long replica telemetry samples are retained.",
            "seconds",
            86_400,
            3_600,
            31_536_000,
            86_400,
            604_800
        ),
        setting!(
            "replicas.sample_window_seconds",
            "Replicas",
            "Default telemetry window",
            "Default history window returned when the client supplies none.",
            "seconds",
            3_600,
            60,
            2_592_000,
            900,
            86_400
        ),
        setting!(
            "replicas.sample_max_points",
            "Replicas",
            "Telemetry point limit",
            "Maximum samples returned for one replica history request.",
            "points",
            1_000,
            10,
            100_000,
            100,
            5_000
        ),
    ]
}

impl ServerSettings {
    pub fn validate(&self) -> Result<(), String> {
        for definition in server_setting_catalog() {
            let value = self
                .value(&definition.key)
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

    fn value(&self, key: &str) -> Option<u64> {
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
            "replicas.stale_after_seconds" => self.replicas.stale_after_seconds,
            "replicas.reap_after_seconds" => self.replicas.reap_after_seconds,
            "replicas.delete_after_seconds" => self.replicas.delete_after_seconds,
            "replicas.reaper_interval_seconds" => self.replicas.reaper_interval_seconds,
            "replicas.sample_retention_seconds" => self.replicas.sample_retention_seconds,
            "replicas.sample_window_seconds" => self.replicas.sample_window_seconds,
            "replicas.sample_max_points" => self.replicas.sample_max_points,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid_and_every_catalog_key_resolves() {
        let settings = ServerSettings::default();
        assert!(settings.validate().is_ok());
        assert!(
            server_setting_catalog()
                .iter()
                .all(|item| settings.value(item.key).is_some())
        );
    }

    #[test]
    fn relational_replica_windows_are_validated() {
        let mut settings = ServerSettings::default();
        settings.replicas.reap_after_seconds = settings.replicas.stale_after_seconds;
        assert!(
            settings
                .validate()
                .unwrap_err()
                .contains("reap_after_seconds")
        );
    }
}
