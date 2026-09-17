//! Typed, platform-wide operating policy.
//!
//! These values are deliberately separate from process bootstrap configuration (addresses,
//! credentials, backend selection, and command-line defaults) and from protocol/safety constants.
//! The catalog is returned with the current values so every administrative client renders the
//! same bounds, defaults, units, and operator guidance that the server validates.

use serde::{Deserialize, Serialize};

use crate::validation::{Validate, ValidationError};

use crate::settings::SettingKind;

pub const SERVER_SETTINGS_SCOPE: &str = "server";
pub const SERVER_SETTINGS_NAME: &str = "operational_policy";

pub fn is_reserved_server_setting(kind: SettingKind, scope: &str, name: &str) -> bool {
    kind == SettingKind::Config
        && ((scope == SERVER_SETTINGS_SCOPE && name == SERVER_SETTINGS_NAME)
            || (scope == crate::billing::AI_RATE_CARD_SCOPE
                && name == crate::billing::AI_RATE_CARD_NAME))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerSettingKind {
    Integer,
    Boolean,
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
            kind: ServerSettingKind::Integer,
            default: $default,
            minimum: $min,
            maximum: $max,
            usual_minimum: $usual_min,
            usual_maximum: $usual_max,
        }
    };
}

macro_rules! boolean_setting {
    ($key:literal, $section:literal, $label:literal, $description:literal, $default:expr) => {
        ServerSettingDefinition {
            key: $key,
            section: $section,
            label: $label,
            description: $description,
            unit: "",
            kind: ServerSettingKind::Boolean,
            default: u64::from($default),
            minimum: 0,
            maximum: 1,
            usual_minimum: 0,
            usual_maximum: 1,
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
            "orchestration.correlated_reducer_poll_interval_ms",
            "Orchestration",
            "Correlated reducer poll interval",
            "Backstop delay between correlated-binding and internal command outbox scans.",
            "ms",
            250,
            10,
            10_000,
            50,
            1_000
        ),
        setting!(
            "orchestration.correlated_reducer_lease_seconds",
            "Orchestration",
            "Correlated reducer lease",
            "Lease held while one engine replica reduces a binding or executes its internal command.",
            "seconds",
            60,
            5,
            3_600,
            30,
            300
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
            "orchestration.adapter_diagnostic_retention_seconds",
            "Orchestration",
            "Adapter diagnostic retention",
            "How long terminal adapter deliveries and polling attempts remain available in Command Center. Set to zero to disable pruning.",
            "seconds",
            604_800,
            0,
            31_536_000,
            86_400,
            2_592_000
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
            "workers.max_concurrent_actions",
            "Workers",
            "Maximum concurrent actions",
            "Maximum provider actions each standalone worker executes at once.",
            "actions",
            4,
            1,
            1_024,
            1,
            32
        ),
        setting!(
            "workers.shutdown_grace_seconds",
            "Workers",
            "Shutdown grace",
            "Time a standalone worker allows in-flight actions to finish while restarting or stopping.",
            "seconds",
            30,
            1,
            3_600,
            10,
            300
        ),
        setting!(
            "workers.reconnect_max_attempts",
            "Workers",
            "Reconnect attempts",
            "Consecutive connection failures tolerated before a standalone worker exits. Zero retries indefinitely.",
            "attempts",
            0,
            0,
            100_000,
            0,
            100
        ),
        setting!(
            "workers.settings_refresh_interval_seconds",
            "Workers",
            "Settings refresh interval",
            "Maximum delay before standalone workers check for updated worker policy.",
            "seconds",
            5,
            1,
            300,
            2,
            30
        ),
        setting!(
            "wakers.max_concurrent_wakes",
            "Wakers",
            "Maximum concurrent wakes",
            "Maximum timer wakes each waker handles at once.",
            "wakes",
            32,
            1,
            4_096,
            8,
            128
        ),
        setting!(
            "wakers.max_wake_sleep_seconds",
            "Wakers",
            "Maximum wake sleep",
            "Longest time a waker holds a not-yet-due delivery before returning it to the broker for re-evaluation.",
            "seconds",
            20,
            1,
            300,
            5,
            25
        ),
        setting!(
            "background_engine.max_concurrent_ingress",
            "Engine Workers",
            "Maximum concurrent ingress",
            "Maximum ingress deliveries each durable engine runtime applies at once.",
            "deliveries",
            16,
            1,
            1_024,
            4,
            64
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
        setting!(
            "archiver.interval_seconds",
            "Archiver",
            "Pass interval",
            "Delay between retention passes. Archiver replicas re-read this policy while waiting.",
            "seconds",
            3_600,
            10,
            604_800,
            60,
            86_400
        ),
        setting!(
            "archiver.claim_lease_seconds",
            "Archiver",
            "Claim lease",
            "Lease held while one archiver fetches, writes, and deletes a claimed archive batch.",
            "seconds",
            600,
            10,
            86_400,
            60,
            3_600
        ),
        setting!(
            "archiver.batch_size",
            "Archiver",
            "Batch size",
            "Maximum rows marked or claimed for one table in a retention batch.",
            "rows",
            1_000,
            1,
            100_000,
            100,
            10_000
        ),
        boolean_setting!(
            "archiver.dry_run",
            "Archiver",
            "Dry run",
            "Discover eligible rows without writing archives or deleting source data.",
            false
        ),
        setting!(
            "archiver.workflow_run_retention_seconds",
            "Archiver",
            "Workflow run retention",
            "Retention for terminal workflow runs, task runs, files, and VM history. Zero disables this policy.",
            "seconds",
            7_776_000,
            0,
            315_360_000,
            604_800,
            31_536_000
        ),
        setting!(
            "archiver.pipeline_run_retention_seconds",
            "Archiver",
            "Pipeline run retention",
            "Retention for terminal pipeline runs, member attempts, and trigger firings. Zero disables this policy.",
            "seconds",
            7_776_000,
            0,
            315_360_000,
            604_800,
            31_536_000
        ),
        setting!(
            "archiver.orchestration_retention_seconds",
            "Archiver",
            "Correlated orchestration retention",
            "Retention for terminal ingress admissions and their correlated orchestration history. Zero disables this policy.",
            "seconds",
            7_776_000,
            0,
            315_360_000,
            604_800,
            31_536_000
        ),
        setting!(
            "archiver.effect_dispatch_retention_seconds",
            "Archiver",
            "Effect dispatch retention",
            "Retention for published or permanently failed effect outbox rows. Zero disables this policy.",
            "seconds",
            604_800,
            0,
            31_536_000,
            86_400,
            2_592_000
        ),
        setting!(
            "archiver.notification_retention_seconds",
            "Archiver",
            "Notification retention",
            "Retention for notifications and settled delivery attempts. Zero disables this policy.",
            "seconds",
            2_592_000,
            0,
            315_360_000,
            604_800,
            7_776_000
        ),
        setting!(
            "archiver.dead_letter_retention_seconds",
            "Archiver",
            "Dead-letter retention",
            "Retention for broker dead letters. Zero disables this policy.",
            "seconds",
            7_776_000,
            0,
            315_360_000,
            604_800,
            31_536_000
        ),
        setting!(
            "archiver.audit_log_retention_seconds",
            "Archiver",
            "Audit-log retention",
            "Retention for authorization and sensitive-operation audit records. Zero disables this policy.",
            "seconds",
            31_536_000,
            0,
            630_720_000,
            7_776_000,
            157_680_000
        ),
        setting!(
            "archiver.idempotency_retention_seconds",
            "Archiver",
            "Idempotency retention",
            "Retention for completed or legacy idempotency keys. Zero disables this policy.",
            "seconds",
            604_800,
            0,
            31_536_000,
            86_400,
            2_592_000
        ),
        setting!(
            "archiver.automation_retention_seconds",
            "Archiver",
            "Automation retention",
            "Retention for resolved automation records and gates. Zero disables this policy.",
            "seconds",
            7_776_000,
            0,
            315_360_000,
            604_800,
            31_536_000
        ),
        setting!(
            "archiver.usage_retention_seconds",
            "Archiver",
            "Usage-ledger retention",
            "Retention for organization resource-usage samples. Zero disables this policy.",
            "seconds",
            31_536_000,
            0,
            630_720_000,
            2_592_000,
            157_680_000
        ),
        setting!(
            "archiver.revision_retention_seconds",
            "Archiver",
            "Revision retention",
            "Retention for superseded workflow and pipeline revisions; the newest revision is always kept. Zero disables this policy.",
            "seconds",
            31_536_000,
            0,
            630_720_000,
            2_592_000,
            157_680_000
        ),
        setting!(
            "archiver.agent_directive_retention_seconds",
            "Archiver",
            "Agent-directive retention",
            "Retention for completed, failed, unsupported, and expired agent directives. Zero disables this policy.",
            "seconds",
            2_592_000,
            0,
            315_360_000,
            604_800,
            7_776_000
        ),
        setting!(
            "archiver.archive_ledger_retention_seconds",
            "Archiver",
            "Archive-ledger retention",
            "Retention for completed archive marks after their source rows are removed. Zero disables this policy.",
            "seconds",
            2_592_000,
            0,
            31_536_000,
            604_800,
            7_776_000
        ),
        setting!(
            "archiver.security_retention_seconds",
            "Archiver",
            "Expired security-state retention",
            "Grace period before expired or revoked sessions and enrollment tokens are purged. Zero disables this policy.",
            "seconds",
            604_800,
            0,
            31_536_000,
            86_400,
            2_592_000
        ),
        setting!(
            "archiver.coordination_retention_seconds",
            "Archiver",
            "Coordination-state retention",
            "Retention for inactive workflow cooldown and mutex keys. Zero disables this policy.",
            "seconds",
            2_592_000,
            0,
            31_536_000,
            604_800,
            7_776_000
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid_and_every_catalog_key_resolves() {
        let settings = ServerSettings::default();
        assert!(settings.validate().is_ok());
        assert!(server_setting_catalog().iter().all(|item| match item.kind {
            ServerSettingKind::Integer => settings.integer_value(item.key).is_some(),
            ServerSettingKind::Boolean => settings.boolean_value(item.key).is_some(),
        }));
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

    #[test]
    fn older_policy_documents_receive_worker_defaults() {
        let settings: ServerSettings = serde_json::from_str("{}").unwrap();

        assert_eq!(settings.workers, WorkerSettings::default());
    }

    #[test]
    fn worker_capacity_uses_the_catalog_bounds() {
        let mut settings = ServerSettings::default();
        settings.workers.max_concurrent_actions = 0;

        assert!(
            settings
                .validate()
                .unwrap_err()
                .contains("workers.max_concurrent_actions")
        );
    }

    #[test]
    fn waker_and_engine_capacity_use_the_catalog_bounds() {
        let mut settings = ServerSettings::default();
        settings.wakers.max_concurrent_wakes = 0;
        assert!(
            settings
                .validate()
                .unwrap_err()
                .contains("wakers.max_concurrent_wakes")
        );

        let mut settings = ServerSettings::default();
        settings.background_engine.max_concurrent_ingress = 0;
        assert!(
            settings
                .validate()
                .unwrap_err()
                .contains("background_engine.max_concurrent_ingress")
        );
    }

    #[test]
    fn older_policy_documents_receive_waker_and_engine_defaults() {
        let settings: ServerSettings = serde_json::from_str("{}").unwrap();

        assert_eq!(settings.wakers, WakerSettings::default());
        assert_eq!(
            settings.background_engine,
            BackgroundEngineSettings::default()
        );
    }
}

mod server_settings;
pub use server_settings::ServerSettings;

mod authentication_settings;
pub use authentication_settings::AuthenticationSettings;

mod orchestration_settings;
pub use orchestration_settings::OrchestrationSettings;

mod notification_settings;
pub use notification_settings::NotificationSettings;

mod worker_settings;
pub use worker_settings::WorkerSettings;

mod worker_settings_response;
pub use worker_settings_response::WorkerSettingsResponse;

mod waker_settings;
pub use waker_settings::WakerSettings;

mod background_engine_settings;
pub use background_engine_settings::BackgroundEngineSettings;

mod replica_settings;
pub use replica_settings::ReplicaSettings;

mod archiver_settings;
pub use archiver_settings::ArchiverSettings;

mod server_setting_definition;
pub use server_setting_definition::ServerSettingDefinition;

mod runtime_setting_definition;
pub use runtime_setting_definition::RuntimeSettingDefinition;
