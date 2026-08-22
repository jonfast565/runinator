use super::*;
use runinator_models::auth::AuthContext;
use runinator_models::replicas::{
    AgentStatusReport, ReplicaHeartbeatRequest, ReplicaKind, ReplicaListResponse,
    ReplicaProviderRegistration, ReplicaProviderRegistrationRequest, ReplicaRecord,
    ReplicaRegistrationRequest, ReplicaStatus,
};
use runinator_models::telemetry::{ReplicaSample, ReplicaSampleSeries, ResourceTelemetry};
use uuid::Uuid;

// inactivity window after which a replica stops counting as live. shared by replica listing and by
// executor-lease invalidation, so a worker declared dead in the ui is the same one whose lease is
// reclaimable. three missed heartbeats at the 10s worker interval.
pub const REPLICA_STALE_SECONDS: i64 = 30;
// inactivity window after which a stale replica is reaped to offline.
pub const DEFAULT_REPLICA_REAP_SECONDS: i64 = 600;
// inactivity window after which an offline replica row is hard-deleted (60 minutes).
pub const DEFAULT_REPLICA_DELETE_SECONDS: i64 = 3600;
// retention window for telemetry samples; older points are pruned by the reaper. 24 hours.
pub const REPLICA_SAMPLE_RETENTION_SECONDS: i64 = 86_400;
// default window and cap when serving the samples endpoint.
const REPLICA_SAMPLE_DEFAULT_WINDOW_SECONDS: i64 = 3_600;
const REPLICA_SAMPLE_MAX_POINTS: i64 = 1_000;

pub async fn register_replica<T: DatabaseImpl>(
    db: &T,
    request: ReplicaRegistrationRequest,
    observed_ip: Option<String>,
    registered_by: &AuthContext,
) -> Result<ReplicaRecord, SendableError> {
    db.register_replica(request, observed_ip, registered_by)
        .await
}

pub async fn heartbeat_replica<T: DatabaseImpl>(
    db: &T,
    replica_id: Uuid,
    request: ReplicaHeartbeatRequest,
    observed_ip: Option<String>,
) -> Result<Option<ReplicaRecord>, SendableError> {
    // pull the live telemetry snapshot off the heartbeat before the request is consumed, so we can
    // append it to the time-series once the heartbeat is accepted.
    let telemetry = extract_telemetry(&request.attributes);
    let incoming_status = agent_status(&request.attributes);
    if let Some(incoming) = incoming_status.as_ref()
        && let Some(previous) = db.fetch_replica(replica_id).await?
        && let Some(previous_status) = agent_status(&previous.attributes)
        && incoming.heartbeat_seq > previous_status.heartbeat_seq.saturating_add(1)
    {
        log::warn!(
            "agent {replica_id} heartbeat sequence jumped from {} to {}",
            previous_status.heartbeat_seq,
            incoming.heartbeat_seq
        );
    }
    let replica = db
        .heartbeat_replica(replica_id, request, observed_ip)
        .await?;
    if replica.is_some()
        && let Some(telemetry) = telemetry
    {
        let sample = ReplicaSample::from_telemetry(replica_id, &telemetry);
        // sampling is best-effort observability; never fail a heartbeat over it.
        if let Err(err) = db.insert_replica_sample(sample).await {
            log::warn!("failed to record replica sample for {replica_id}: {err}");
        }
    }
    Ok(replica)
}

/// deserialize the `telemetry` snapshot carried under a heartbeat's `attributes`.
fn extract_telemetry(attributes: &runinator_models::value::Value) -> Option<ResourceTelemetry> {
    let telemetry = attributes.get("telemetry")?;
    let raw = serde_json::to_string(telemetry).ok()?;
    serde_json::from_str::<ResourceTelemetry>(&raw).ok()
}

pub async fn fetch_replica_samples<T: DatabaseImpl>(
    db: &T,
    replica_id: Uuid,
    since_seconds: Option<i64>,
) -> Result<ReplicaSampleSeries, SendableError> {
    let window = since_seconds
        .filter(|value| *value > 0)
        .unwrap_or(REPLICA_SAMPLE_DEFAULT_WINDOW_SECONDS);
    let since = Utc::now() - Duration::seconds(window);
    let samples = db
        .fetch_replica_samples(replica_id, since, REPLICA_SAMPLE_MAX_POINTS)
        .await?;
    Ok(ReplicaSampleSeries {
        replica_id,
        samples,
    })
}

pub async fn prune_replica_samples<T: DatabaseImpl>(db: &T) -> Result<u64, SendableError> {
    let cutoff = Utc::now() - Duration::seconds(REPLICA_SAMPLE_RETENTION_SECONDS);
    db.prune_replica_samples(cutoff).await
}

pub async fn mark_replica_offline<T: DatabaseImpl>(
    db: &T,
    replica_id: Uuid,
    runtime_id: String,
) -> Result<Option<ReplicaRecord>, SendableError> {
    db.mark_replica_offline(replica_id, runtime_id).await
}

pub async fn reap_inactive_replicas<T: DatabaseImpl>(db: &T) -> Result<u64, SendableError> {
    let cutoff = Utc::now() - Duration::seconds(replica_reap_seconds());
    db.reap_inactive_replicas(cutoff).await
}

pub async fn delete_expired_replicas<T: DatabaseImpl>(db: &T) -> Result<u64, SendableError> {
    let cutoff = Utc::now() - Duration::seconds(replica_delete_seconds());
    db.delete_expired_replicas(cutoff).await
}

pub async fn fetch_replica<T: DatabaseImpl>(
    db: &T,
    replica_id: Uuid,
) -> Result<Option<ReplicaRecord>, SendableError> {
    db.fetch_replica(replica_id).await
}

pub async fn fetch_replica_by_runtime<T: DatabaseImpl>(
    db: &T,
    instance_id: String,
    runtime_id: String,
) -> Result<Option<ReplicaRecord>, SendableError> {
    db.fetch_replica_by_runtime(instance_id, runtime_id).await
}

pub async fn fetch_replicas<T: DatabaseImpl>(
    db: &T,
    replica_type: Option<ReplicaKind>,
    status: Option<ReplicaStatus>,
) -> Result<ReplicaListResponse, SendableError> {
    let stale_before = Utc::now() - Duration::seconds(REPLICA_STALE_SECONDS);
    // fetch every status first: an agent may advertise a longer stale window than the platform
    // default, so a row the database provisionally classified as stale may still be live.
    let mut replicas = db.fetch_replicas(replica_type, None, stale_before).await?;
    apply_advertised_stale_windows(&mut replicas, Utc::now());
    if let Some(status) = status {
        replicas.retain(|replica| replica.status == status);
    }
    let running_tasks = db
        .count_running_effects_by_executor()
        .await?
        .into_iter()
        .collect::<std::collections::HashMap<Uuid, i64>>();
    let counts = runinator_models::replicas::ReplicaCounts {
        workers: replicas
            .iter()
            .filter(|replica| {
                replica.status == ReplicaStatus::Live && replica.replica_type == ReplicaKind::Worker
            })
            .count() as i64,
        wakers: replicas
            .iter()
            .filter(|replica| {
                replica.status == ReplicaStatus::Live && replica.replica_type == ReplicaKind::Waker
            })
            .count() as i64,
        webservices: replicas
            .iter()
            .filter(|replica| {
                replica.status == ReplicaStatus::Live
                    && replica.replica_type == ReplicaKind::Webservice
            })
            .count() as i64,
        background: replicas
            .iter()
            .filter(|replica| {
                replica.status == ReplicaStatus::Live
                    && replica.replica_type == ReplicaKind::Background
            })
            .count() as i64,
    };
    Ok(ReplicaListResponse {
        counts,
        replicas,
        running_tasks,
    })
}

pub fn replica_reap_seconds() -> i64 {
    configured_seconds(
        "RUNINATOR_REPLICA_REAP_SECONDS",
        DEFAULT_REPLICA_REAP_SECONDS,
    )
}

pub fn replica_delete_seconds() -> i64 {
    configured_seconds(
        "RUNINATOR_REPLICA_DELETE_SECONDS",
        DEFAULT_REPLICA_DELETE_SECONDS,
    )
}

fn configured_seconds(name: &str, default: i64) -> i64 {
    std::env::var(name)
        .ok()
        .and_then(|raw| raw.parse::<i64>().ok())
        .filter(|seconds| *seconds > 0)
        .unwrap_or(default)
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

pub async fn upsert_replica_provider_registration<T: DatabaseImpl>(
    db: &T,
    replica_id: Uuid,
    request: ReplicaProviderRegistrationRequest,
) -> Result<ReplicaProviderRegistration, SendableError> {
    db.upsert_replica_provider_registration(replica_id, request)
        .await
}

pub async fn fetch_replica_provider_registrations<T: DatabaseImpl>(
    db: &T,
    replica_id: Uuid,
) -> Result<Vec<ReplicaProviderRegistration>, SendableError> {
    db.fetch_replica_provider_registrations(replica_id).await
}
