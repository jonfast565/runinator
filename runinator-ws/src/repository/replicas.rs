use super::*;
use runinator_models::replicas::{
    ReplicaHeartbeatRequest, ReplicaKind, ReplicaListResponse, ReplicaProviderRegistration,
    ReplicaProviderRegistrationRequest, ReplicaRecord, ReplicaRegistrationRequest, ReplicaStatus,
};
use runinator_models::telemetry::{ReplicaSample, ReplicaSampleSeries, ResourceTelemetry};
use uuid::Uuid;

const REPLICA_STALE_SECONDS: i64 = 30;
// inactivity window after which a stale replica is reaped to offline.
pub(crate) const REPLICA_REAP_SECONDS: i64 = 600;
// inactivity window after which an offline replica row is hard-deleted (60 minutes).
pub(crate) const REPLICA_DELETE_SECONDS: i64 = 3600;
// retention window for telemetry samples; older points are pruned by the reaper. 24 hours.
pub(crate) const REPLICA_SAMPLE_RETENTION_SECONDS: i64 = 86_400;
// default window and cap when serving the samples endpoint.
const REPLICA_SAMPLE_DEFAULT_WINDOW_SECONDS: i64 = 3_600;
const REPLICA_SAMPLE_MAX_POINTS: i64 = 1_000;

pub async fn register_replica<T: DatabaseImpl>(
    db: &T,
    request: ReplicaRegistrationRequest,
    observed_ip: Option<String>,
) -> Result<ReplicaRecord, SendableError> {
    db.register_replica(request, observed_ip).await
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
    let replica = db
        .heartbeat_replica(replica_id, request, observed_ip)
        .await?;
    if replica.is_some() {
        if let Some(telemetry) = telemetry {
            let sample = ReplicaSample::from_telemetry(replica_id, &telemetry);
            // sampling is best-effort observability; never fail a heartbeat over it.
            if let Err(err) = db.insert_replica_sample(sample).await {
                log::warn!("failed to record replica sample for {replica_id}: {err}");
            }
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
    let cutoff = Utc::now() - Duration::seconds(REPLICA_REAP_SECONDS);
    db.reap_inactive_replicas(cutoff).await
}

pub async fn delete_expired_replicas<T: DatabaseImpl>(db: &T) -> Result<u64, SendableError> {
    let cutoff = Utc::now() - Duration::seconds(REPLICA_DELETE_SECONDS);
    db.delete_expired_replicas(cutoff).await
}

pub async fn fetch_replicas<T: DatabaseImpl>(
    db: &T,
    replica_type: Option<ReplicaKind>,
    status: Option<ReplicaStatus>,
) -> Result<ReplicaListResponse, SendableError> {
    let stale_before = Utc::now() - Duration::seconds(REPLICA_STALE_SECONDS);
    let replicas = db
        .fetch_replicas(replica_type, status, stale_before)
        .await?;
    let running_tasks = db
        .count_running_node_runs_by_executor()
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
    };
    Ok(ReplicaListResponse {
        counts,
        replicas,
        running_tasks,
    })
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
