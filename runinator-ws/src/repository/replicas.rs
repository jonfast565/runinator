use super::*;
use runinator_models::replicas::{
    ReplicaHeartbeatRequest, ReplicaKind, ReplicaListResponse, ReplicaProviderRegistration,
    ReplicaProviderRegistrationRequest, ReplicaRecord, ReplicaRegistrationRequest, ReplicaStatus,
};
use uuid::Uuid;

const REPLICA_STALE_SECONDS: i64 = 30;

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
    db.heartbeat_replica(replica_id, request, observed_ip).await
}

pub async fn mark_replica_offline<T: DatabaseImpl>(
    db: &T,
    replica_id: Uuid,
    runtime_id: String,
) -> Result<Option<ReplicaRecord>, SendableError> {
    db.mark_replica_offline(replica_id, runtime_id).await
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
    Ok(ReplicaListResponse { counts, replicas })
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
