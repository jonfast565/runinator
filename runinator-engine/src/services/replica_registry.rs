//! replica registration, liveness, telemetry, and directive application service.

use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use runinator_comm::{AgentDirectiveKind, AgentDirectiveRecord, ReplicaAvailability};
use runinator_models::{
    auth::AuthContext,
    errors::SendableError,
    replicas::{
        AgentStatusReport, ReplicaHeartbeatRequest, ReplicaKind, ReplicaListResponse,
        ReplicaProviderRegistration, ReplicaProviderRegistrationRequest, ReplicaRecord,
        ReplicaRegistrationRequest, ReplicaStatus,
    },
    telemetry::{ReplicaSample, ReplicaSampleSeries, ResourceTelemetry},
};
use runinator_store::roles::ReplicaStore;
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
pub struct ReplicaRegistry<T> {
    store: Arc<T>,
}

impl<T> Clone for ReplicaRegistry<T> {
    fn clone(&self) -> Self {
        Self {
            store: self.store.clone(),
        }
    }
}

impl<T> ReplicaRegistry<T> {
    /// Create a registry over one replica-store implementation.
    pub fn new(store: Arc<T>) -> Self {
        Self { store }
    }
}

impl<T: ReplicaStore> ReplicaRegistry<T> {
    /// Register or refresh a replica while preserving its original owner.
    pub async fn register(
        &self,
        request: ReplicaRegistrationRequest,
        observed_ip: Option<String>,
        registered_by: &AuthContext,
    ) -> Result<ReplicaRecord, SendableError> {
        self.store
            .register_replica(request, observed_ip, registered_by)
            .await
    }

    /// Apply a heartbeat and best-effort persist its telemetry snapshot.
    pub async fn heartbeat(
        &self,
        replica_id: Uuid,
        request: ReplicaHeartbeatRequest,
        observed_ip: Option<String>,
    ) -> Result<Option<ReplicaRecord>, SendableError> {
        // pull the live telemetry snapshot off the heartbeat before the request is consumed, so we
        // can append it to the time-series once the heartbeat is accepted.
        let telemetry = extract_telemetry(&request.attributes);
        let incoming_status = agent_status(&request.attributes);
        if let Some(incoming) = incoming_status.as_ref()
            && let Some(previous) = self.store.fetch_replica(replica_id).await?
            && let Some(previous_status) = agent_status(&previous.attributes)
            && incoming.heartbeat_seq > previous_status.heartbeat_seq.saturating_add(1)
        {
            log::warn!(
                "agent {replica_id} heartbeat sequence jumped from {} to {}",
                previous_status.heartbeat_seq,
                incoming.heartbeat_seq
            );
        }
        let replica = self
            .store
            .heartbeat_replica(replica_id, request, observed_ip)
            .await?;
        if replica.is_some() {
            self.record_telemetry(replica_id, telemetry).await;
        }
        Ok(replica)
    }

    /// Persist a lifecycle observation received through the broker ingress channel.
    ///
    /// Workers, wakers, background engines, and archivers all use this path. Their availability
    /// is therefore recorded by the same registry service as HTTP registrations, without making a
    /// data-plane process depend on the web-service API.
    pub async fn observe_broker_availability(
        &self,
        availability: ReplicaAvailability,
    ) -> Result<(), SendableError> {
        match availability {
            ReplicaAvailability::Available {
                registration,
                providers,
            } => {
                match registration.replica_type {
                    ReplicaKind::Worker
                    | ReplicaKind::Waker
                    | ReplicaKind::Background
                    | ReplicaKind::Archiver => {}
                    ReplicaKind::Webservice | ReplicaKind::Postgres => {
                        return Err(format!(
                            "{} replicas must register directly with the web service",
                            registration.replica_type.as_str()
                        )
                        .into());
                    }
                }
                let expected_id = registration.replica_id.ok_or_else(|| {
                    "broker-announced replica availability requires a replica_id".to_string()
                })?;
                let telemetry = extract_telemetry(&registration.attributes);
                let runtime_id = registration.runtime_id.clone();
                let replica = self
                    .register(registration, None, &AuthContext::disabled_platform_admin())
                    .await?;
                if replica.replica_id != expected_id {
                    return Err(format!(
                        "broker-announced replica {expected_id} resolved to {}, refusing mismatched identity",
                        replica.replica_id
                    )
                    .into());
                }
                self.record_telemetry(replica.replica_id, telemetry).await;
                for provider in providers {
                    self.upsert_provider(
                        replica.replica_id,
                        ReplicaProviderRegistrationRequest {
                            runtime_id: runtime_id.clone(),
                            provider,
                        },
                    )
                    .await?;
                }
                Ok(())
            }
            ReplicaAvailability::Offline {
                replica_id,
                runtime_id,
            } => {
                self.mark_offline(replica_id, runtime_id).await?;
                Ok(())
            }
        }
    }

    /// Mark a replica offline if the supplied runtime identity still matches.
    pub async fn mark_offline(
        &self,
        replica_id: Uuid,
        runtime_id: String,
    ) -> Result<Option<ReplicaRecord>, SendableError> {
        self.store
            .mark_replica_offline(replica_id, runtime_id)
            .await
    }

    /// Fetch a replica by its durable id.
    pub async fn fetch(&self, replica_id: Uuid) -> Result<Option<ReplicaRecord>, SendableError> {
        self.store.fetch_replica(replica_id).await
    }

    /// Fetch the replica a registration would replace.
    pub async fn fetch_by_runtime(
        &self,
        instance_id: String,
        runtime_id: String,
    ) -> Result<Option<ReplicaRecord>, SendableError> {
        self.store
            .fetch_replica_by_runtime(instance_id, runtime_id)
            .await
    }

    /// Check whether an agent principal owns an existing replica. Other principal kinds are not
    /// replica-bound and leave authorization to the outer capability layer.
    pub async fn agent_owns_replica(
        &self,
        context: &AuthContext,
        replica_id: Uuid,
    ) -> Result<bool, SendableError> {
        if !matches!(
            context.system_role,
            Some(
                runinator_models::rbac::SystemRole::Agent
                    | runinator_models::rbac::SystemRole::Replica
            )
        ) {
            return Ok(true);
        }
        Ok(matches!(
            self.fetch(replica_id).await?,
            Some(replica) if replica.registered_by_principal_id == context.principal_id
        ))
    }

    /// Check whether an agent principal may replace an existing runtime registration.
    pub async fn agent_owns_runtime_registration(
        &self,
        context: &AuthContext,
        request: &ReplicaRegistrationRequest,
    ) -> Result<bool, SendableError> {
        if !matches!(
            context.system_role,
            Some(
                runinator_models::rbac::SystemRole::Agent
                    | runinator_models::rbac::SystemRole::Replica
            )
        ) {
            return Ok(true);
        }
        Ok(!matches!(
            self.fetch_by_runtime(request.instance_id.clone(), request.runtime_id.clone())
                .await?,
            Some(replica) if replica.registered_by_principal_id != context.principal_id
        ))
    }

    /// List replicas with liveness and running-effect counts derived for the operator surface.
    pub async fn list(
        &self,
        replica_type: Option<ReplicaKind>,
        status: Option<ReplicaStatus>,
    ) -> Result<ReplicaListResponse, SendableError> {
        let stale_before = Utc::now() - Duration::seconds(REPLICA_STALE_SECONDS);
        // fetch every status first: an agent may advertise a longer stale window than the platform
        // default, so a row the database provisionally classified as stale may still be live.
        let mut replicas = self
            .store
            .fetch_replicas(replica_type, None, stale_before)
            .await?;
        apply_advertised_stale_windows(&mut replicas, Utc::now());
        if let Some(status) = status {
            replicas.retain(|replica| replica.status == status);
        }
        let running_tasks = self
            .store
            .count_running_effects_by_executor()
            .await?
            .into_iter()
            .collect();
        let counts = runinator_models::replicas::ReplicaCounts {
            workers: live_count(&replicas, ReplicaKind::Worker),
            wakers: live_count(&replicas, ReplicaKind::Waker),
            webservices: live_count(&replicas, ReplicaKind::Webservice),
            background: live_count(&replicas, ReplicaKind::Background),
        };
        Ok(ReplicaListResponse {
            counts,
            replicas,
            running_tasks,
        })
    }

    /// Fetch a bounded recent telemetry window for one replica.
    pub async fn samples(
        &self,
        replica_id: Uuid,
        since_seconds: Option<i64>,
    ) -> Result<ReplicaSampleSeries, SendableError> {
        let window = since_seconds
            .filter(|value| *value > 0)
            .unwrap_or(REPLICA_SAMPLE_DEFAULT_WINDOW_SECONDS);
        let since = Utc::now() - Duration::seconds(window);
        let samples = self
            .store
            .fetch_replica_samples(replica_id, since, REPLICA_SAMPLE_MAX_POINTS)
            .await?;
        Ok(ReplicaSampleSeries {
            replica_id,
            samples,
        })
    }

    /// Register the provider metadata advertised by one replica.
    pub async fn upsert_provider(
        &self,
        replica_id: Uuid,
        request: ReplicaProviderRegistrationRequest,
    ) -> Result<ReplicaProviderRegistration, SendableError> {
        self.store
            .upsert_replica_provider_registration(replica_id, request)
            .await
    }

    /// Fetch provider metadata advertised by one replica.
    pub async fn providers(
        &self,
        replica_id: Uuid,
    ) -> Result<Vec<ReplicaProviderRegistration>, SendableError> {
        self.store
            .fetch_replica_provider_registrations(replica_id)
            .await
    }

    async fn record_telemetry(&self, replica_id: Uuid, telemetry: Option<ResourceTelemetry>) {
        let Some(telemetry) = telemetry else {
            return;
        };
        let sample = ReplicaSample::from_telemetry(replica_id, &telemetry);
        // Sampling is best-effort observability; never fail liveness over it.
        if let Err(err) = self.store.insert_replica_sample(sample).await {
            log::warn!("failed to record replica sample for {replica_id}: {err}");
        }
    }

    /// Enqueue a replica directive. Transport hints are emitted by the caller after this durable
    /// write succeeds, so this service remains independent of WebSockets, broker publication, and
    /// an embedded engine's process-local latency signals.
    pub async fn issue_directive(
        &self,
        replica_id: Uuid,
        kind: AgentDirectiveKind,
        expires_at: DateTime<Utc>,
    ) -> Result<Option<AgentDirectiveRecord>, SendableError> {
        if self.fetch(replica_id).await?.is_none() {
            return Ok(None);
        }
        let record = self
            .store
            .enqueue_agent_directive(replica_id, kind, expires_at)
            .await?;
        Ok(Some(record))
    }

    /// List the durable directive history for one replica.
    pub async fn directives(
        &self,
        replica_id: Uuid,
        limit: i64,
    ) -> Result<Vec<AgentDirectiveRecord>, SendableError> {
        self.store.list_agent_directives(replica_id, limit).await
    }

    /// Reap replicas that have stayed inactive past the configured liveness window.
    pub async fn reap_inactive(&self) -> Result<u64, SendableError> {
        let cutoff = Utc::now() - Duration::seconds(replica_reap_seconds());
        self.store.reap_inactive_replicas(cutoff).await
    }

    /// Remove replicas that have remained offline past the configured retention window.
    pub async fn delete_expired(&self) -> Result<u64, SendableError> {
        let cutoff = Utc::now() - Duration::seconds(replica_delete_seconds());
        self.store.delete_expired_replicas(cutoff).await
    }

    /// Remove telemetry samples outside the fixed retention window.
    pub async fn prune_samples(&self) -> Result<u64, SendableError> {
        let cutoff = Utc::now() - Duration::seconds(REPLICA_SAMPLE_RETENTION_SECONDS);
        self.store.prune_replica_samples(cutoff).await
    }
}

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
    std::env::var(name)
        .ok()
        .and_then(|raw| raw.parse::<i64>().ok())
        .filter(|seconds| *seconds > 0)
        .unwrap_or(default)
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
