//! the fleet: replica registration, heartbeats, reaping, telemetry samples, and provider registrations.
//!
//! one of the role traits `DatabaseImpl` composes. bound on this directly when a caller only
//! needs this slice of the store.

use std::future::Future;

use super::QueueSnapshot;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use runinator_comm::{AgentDirectiveKind, AgentDirectiveRecord, AgentDirectiveResult};
use runinator_models::{
    auth::AuthContext,
    errors::SendableError,
    replicas::{
        ReplicaHeartbeatRequest, ReplicaProviderRegistration, ReplicaProviderRegistrationRequest,
        ReplicaRecord, ReplicaRegistrationRequest,
    },
    telemetry::ReplicaSample,
};

/// Core persistence operations for Runinator.
/// The fleet: replica registration, heartbeats, reaping, telemetry samples, and provider registrations.
pub trait ReplicaStore: Send + Sync + 'static {
    /// Operational snapshot of due, incomplete agent directives.
    fn agent_directive_queue_snapshot(
        &self,
        now: DateTime<Utc>,
        stale_before: DateTime<Utc>,
    ) -> impl Future<Output = Result<QueueSnapshot, SendableError>> + Send;

    /// Persist a replica-scoped directive before it is offered to the broker.
    fn enqueue_agent_directive(
        &self,
        replica_id: Uuid,
        kind: AgentDirectiveKind,
        expires_at: DateTime<Utc>,
    ) -> impl Future<Output = Result<AgentDirectiveRecord, SendableError>> + Send;

    /// Atomically claim directives due for initial publication or redelivery.
    fn claim_due_agent_directives(
        &self,
        runtime_id: String,
        now: DateTime<Utc>,
        stale_before: DateTime<Utc>,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<AgentDirectiveRecord>, SendableError>> + Send;

    /// Record that a claimed directive was published successfully.
    fn mark_agent_directive_published(
        &self,
        directive_id: Uuid,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Settle a directive from the result returned by its target agent.
    fn complete_agent_directive(
        &self,
        result: AgentDirectiveResult,
    ) -> impl Future<Output = Result<Option<AgentDirectiveRecord>, SendableError>> + Send;

    /// Fetch one directive by id.
    fn fetch_agent_directive(
        &self,
        directive_id: Uuid,
    ) -> impl Future<Output = Result<Option<AgentDirectiveRecord>, SendableError>> + Send;

    /// List recent directives for one replica, newest first.
    fn list_agent_directives(
        &self,
        replica_id: Uuid,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<AgentDirectiveRecord>, SendableError>> + Send;

    /// Mark unfinished directives past their deadline as expired.
    fn expire_agent_directives(
        &self,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<u64, SendableError>> + Send;

    /// Register or refresh a runtime replica. `registered_by` is only recorded on the initial
    /// insert (a later re-registration of the same instance_id/runtime_id upserts the rest of the
    /// row but never reassigns ownership).
    fn register_replica(
        &self,
        request: ReplicaRegistrationRequest,
        observed_ip: Option<String>,
        registered_by: &AuthContext,
    ) -> impl Future<Output = Result<ReplicaRecord, SendableError>> + Send;

    /// Refresh a replica heartbeat if the runtime id still matches.
    fn heartbeat_replica(
        &self,
        replica_id: Uuid,
        request: ReplicaHeartbeatRequest,
        observed_ip: Option<String>,
    ) -> impl Future<Output = Result<Option<ReplicaRecord>, SendableError>> + Send;

    /// Mark a replica offline if the runtime id still matches.
    fn mark_replica_offline(
        &self,
        replica_id: Uuid,
        runtime_id: String,
    ) -> impl Future<Output = Result<Option<ReplicaRecord>, SendableError>> + Send;

    /// Permanently end one runtime activation. The machine enrollment remains valid, but this
    /// replica id can no longer heartbeat or re-register.
    fn kick_replica(
        &self,
        replica_id: Uuid,
        kicked_at: DateTime<Utc>,
    ) -> impl Future<Output = Result<Option<ReplicaRecord>, SendableError>> + Send;

    /// Kick every current or historical activation owned by one enrolled machine principal.
    fn kick_replicas_by_principal(
        &self,
        principal_id: Uuid,
        kicked_at: DateTime<Utc>,
    ) -> impl Future<Output = Result<u64, SendableError>> + Send;

    /// Mark replicas offline that have not sent a heartbeat since the cutoff. returns the count
    /// reaped so callers can log activity.
    fn reap_inactive_replicas(
        &self,
        cutoff: DateTime<Utc>,
    ) -> impl Future<Output = Result<u64, SendableError>> + Send;

    /// Hard-delete replicas whose last heartbeat predates the cutoff once independently retained
    /// telemetry, directives, and historical attribution no longer reference them. returns the
    /// count purged.
    fn delete_expired_replicas(
        &self,
        cutoff: DateTime<Utc>,
    ) -> impl Future<Output = Result<u64, SendableError>> + Send;

    /// Fetch a single replica by id, so a caller presenting a `replica_id` (e.g. over the WS broker
    /// relay) can be checked against who registered it.
    fn fetch_replica(
        &self,
        replica_id: Uuid,
    ) -> impl Future<Output = Result<Option<ReplicaRecord>, SendableError>> + Send;

    /// Fetch the row a registration upsert would touch, so a lower-trust principal can be checked
    /// against its immutable owner before any fields are changed.
    fn fetch_replica_by_runtime(
        &self,
        instance_id: String,
        runtime_id: String,
    ) -> impl Future<Output = Result<Option<ReplicaRecord>, SendableError>> + Send;

    /// Fetch replicas filtered by type and status, deriving stale state from heartbeat age.
    fn fetch_replicas(
        &self,
        replica_type: Option<runinator_models::replicas::ReplicaKind>,
        status: Option<runinator_models::replicas::ReplicaStatus>,
        stale_before: DateTime<Utc>,
    ) -> impl Future<Output = Result<Vec<ReplicaRecord>, SendableError>> + Send;

    /// Count effects currently held by each executor replica, keyed by replica id. reflects live
    /// executor claims, so the count is the number of tasks actively running on each worker.
    fn count_running_effects_by_executor(
        &self,
    ) -> impl Future<Output = Result<Vec<(Uuid, i64)>, SendableError>> + Send;

    /// Append a telemetry sample to the replica time-series.
    fn insert_replica_sample(
        &self,
        sample: ReplicaSample,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Fetch a replica's telemetry samples taken at or after `since`, oldest first.
    fn fetch_replica_samples(
        &self,
        replica_id: Uuid,
        since: DateTime<Utc>,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<ReplicaSample>, SendableError>> + Send;

    /// Delete telemetry samples older than `cutoff`. returns the count purged.
    fn prune_replica_samples(
        &self,
        cutoff: DateTime<Utc>,
    ) -> impl Future<Output = Result<u64, SendableError>> + Send;

    /// Upsert a provider registration for a worker replica.
    fn upsert_replica_provider_registration(
        &self,
        replica_id: Uuid,
        request: ReplicaProviderRegistrationRequest,
    ) -> impl Future<Output = Result<ReplicaProviderRegistration, SendableError>> + Send;

    /// Fetch provider registrations for a replica.
    fn fetch_replica_provider_registrations(
        &self,
        replica_id: Uuid,
    ) -> impl Future<Output = Result<Vec<ReplicaProviderRegistration>, SendableError>> + Send;
}
