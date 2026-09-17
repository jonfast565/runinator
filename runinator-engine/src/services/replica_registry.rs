//! replica registration, liveness, telemetry, and directive application service.

use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use runinator_comm::{AgentDirectiveKind, AgentDirectiveRecord, ReplicaAvailability};
use runinator_models::{
    auth::{AgentMachineEnrollment, AuthContext, PrincipalKind},
    errors::SendableError,
    rbac::SystemRole,
    replicas::{
        AgentStatusReport, ReplicaHeartbeatRequest, ReplicaKind, ReplicaListResponse,
        ReplicaProviderRegistration, ReplicaProviderRegistrationRequest, ReplicaRecord,
        ReplicaRegistrationRequest, ReplicaStatus,
    },
    telemetry::{ReplicaSample, ReplicaSampleSeries, ResourceTelemetry},
};
use runinator_platform::env;
use runinator_store::{
    RuntimeStore,
    roles::{AuthStore, RbacStore, ReplicaStore},
};
use uuid::Uuid;

// inactivity window after which a replica stops counting as live. shared by replica listing and
// executor-lease invalidation. three missed heartbeats at the 10s worker interval.
pub const REPLICA_STALE_SECONDS: i64 = 30;
// inactivity window after which a stale replica is reaped to offline.
pub const DEFAULT_REPLICA_REAP_SECONDS: i64 = 600;
// inactivity window after which an offline replica row is hard-deleted (60 minutes).
pub const DEFAULT_REPLICA_DELETE_SECONDS: i64 = 3600;
// retention window for telemetry samples; older points are pruned by the reaper. 24 hours.
pub const REPLICA_SAMPLE_RETENTION_SECONDS: i64 = 86_400;

const REPLICA_SAMPLE_DEFAULT_WINDOW_SECONDS: i64 = 3_600;
const REPLICA_SAMPLE_MAX_POINTS: i64 = 1_000;

/// Coordinates the replica persistence slice used by fleet-facing transports and engine loops.

/// Read the configured inactivity-reap window.
pub fn replica_reap_seconds() -> i64 {
    configured_seconds(
        "RUNINATOR_REPLICA_REAP_SECONDS",
        DEFAULT_REPLICA_REAP_SECONDS,
    )
}

/// Read the configured offline-row retention window.
pub fn replica_delete_seconds() -> i64 {
    configured_seconds(
        "RUNINATOR_REPLICA_DELETE_SECONDS",
        DEFAULT_REPLICA_DELETE_SECONDS,
    )
}

fn configured_seconds(name: &str, default: i64) -> i64 {
    env::parse_positive_or(name, default)
}

fn extract_telemetry(attributes: &runinator_models::value::Value) -> Option<ResourceTelemetry> {
    let telemetry = attributes.get("telemetry")?;
    let raw = serde_json::to_string(telemetry).ok()?;
    serde_json::from_str::<ResourceTelemetry>(&raw).ok()
}

fn apply_advertised_stale_windows(replicas: &mut [ReplicaRecord], now: DateTime<Utc>) {
    for replica in replicas {
        if replica.status != ReplicaStatus::Stale {
            continue;
        }
        let Some(stale_after) = agent_status(&replica.attributes)
            .and_then(|status| status.stale_after_seconds)
            .filter(|seconds| *seconds > REPLICA_STALE_SECONDS as u64)
        else {
            continue;
        };
        if now - replica.last_heartbeat_at < Duration::seconds(stale_after as i64) {
            replica.status = ReplicaStatus::Live;
        }
    }
}

fn agent_status(attributes: &runinator_models::value::Value) -> Option<AgentStatusReport> {
    let value = attributes.get("status")?;
    serde_json::from_value(serde_json::to_value(value).ok()?).ok()
}

fn live_count(replicas: &[ReplicaRecord], replica_type: ReplicaKind) -> i64 {
    replicas
        .iter()
        .filter(|replica| {
            replica.status == ReplicaStatus::Live && replica.replica_type == replica_type
        })
        .count() as i64
}

mod replica_registry;
pub use replica_registry::ReplicaRegistry;

mod agent_machine_invalidation;
pub use agent_machine_invalidation::AgentMachineInvalidation;
