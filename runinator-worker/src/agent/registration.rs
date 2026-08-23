//! replica registration for the shared agent lifecycle.
//!
//! registration is required rather than best effort: an agent that never registers is invisible in
//! the replica registry, cannot heartbeat, and cannot be targeted, so it would run as a phantom.
//! retry with backoff, stay interruptible, and give up loudly once the budget is spent.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use runinator_api::{AsyncApiClient, ReplicaClient, ReplicaServiceConfig, StaticLocator};
use runinator_models::errors::SendableError;
use runinator_models::replicas::ReplicaKind;
use runinator_models::value::{Map, Value};
use runinator_observability::resource_telemetry::{
    TelemetryCollector, attributes_with_host_metadata, attributes_with_telemetry,
};
use tokio::task::JoinHandle;

use crate::agent::config::AgentRuntimeConfig;
use crate::agent::reporter::StatusReporter;
use crate::agent::shutdown::Shutdown;
use crate::agent::status::AgentReportContext;

// registration retry envelope: keep trying while the web service is briefly unreachable, then give
// up so the process exits non-zero and its orchestrator restarts it.
const REGISTER_BASE_BACKOFF: Duration = Duration::from_secs(2);
const REGISTER_MAX_BACKOFF: Duration = Duration::from_secs(30);

/// exponential backoff for the nth registration attempt (1-based), capped at [`REGISTER_MAX_BACKOFF`].
pub fn register_backoff(attempt: u32) -> Duration {
    let factor = 1u32
        .checked_shl(attempt.saturating_sub(1))
        .unwrap_or(u32::MAX);
    REGISTER_BASE_BACKOFF
        .saturating_mul(factor)
        .min(REGISTER_MAX_BACKOFF)
}

/// register this agent as a worker replica, retrying with backoff. `Ok(None)` means shutdown fired
/// during a retry window, which is a clean stop rather than a failure.
pub async fn register_agent_replica(
    api_client: &AsyncApiClient<StaticLocator>,
    config: &AgentRuntimeConfig,
    reporter: &StatusReporter,
    report_context: &AgentReportContext,
    shutdown: &Shutdown,
) -> Result<Option<ReplicaClient<StaticLocator>>, SendableError> {
    let mut service_config = replica_service_config(config);
    insert_status(
        &mut service_config.attributes,
        report_context.report(&reporter.status(), 0, 0),
    );
    let mut attempt = 1;
    loop {
        if shutdown.is_stopping() {
            return Ok(None);
        }
        match ReplicaClient::register(api_client.clone(), service_config.clone()).await {
            Ok(client) => {
                if attempt > 1 {
                    reporter.log(format!("Registered replica after {attempt} attempts."));
                }
                return Ok(Some(client));
            }
            Err(err) if attempt >= config.register_max_attempts => {
                reporter.log(format!(
                    "Failed to register replica after {attempt} attempts, giving up: {err}"
                ));
                return Err(crate::errors::REPLICA_REGISTER.error(err));
            }
            Err(err) => {
                let backoff = register_backoff(attempt);
                reporter.log(format!(
                    "Failed to register replica (attempt {attempt}/{}), retrying in {}s: {err}",
                    config.register_max_attempts,
                    backoff.as_secs()
                ));
                if shutdown.sleep_or_stop(backoff).await {
                    return Ok(None);
                }
                attempt += 1;
            }
        }
    }
}

/// heartbeat with the agent-specific status envelope, updating the clock-skew estimate from each
/// accepted response and marking the replica offline on a clean stop.
pub fn spawn_agent_heartbeat(
    replica_client: ReplicaClient<StaticLocator>,
    config: &AgentRuntimeConfig,
    reporter: Arc<StatusReporter>,
    report_context: Arc<AgentReportContext>,
    telemetry: Option<Arc<TelemetryCollector>>,
    shutdown: Shutdown,
) -> JoinHandle<()> {
    let heartbeat_interval = config.heartbeat_interval;
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(heartbeat_interval);
        let mut heartbeat_seq = 0u64;
        let mut clock_skew_ms = 0i64;
        let shutdown_notify = shutdown.notify();
        loop {
            if shutdown.is_stopping() {
                mark_offline(&replica_client, &reporter).await;
                return;
            }
            tokio::select! {
                _ = shutdown_notify.notified() => {
                    mark_offline(&replica_client, &reporter).await;
                    return;
                }
                _ = ticker.tick() => {
                    heartbeat_seq = heartbeat_seq.saturating_add(1);
                    let mut request = replica_client.session.heartbeat_request();
                    if let Some(collector) = telemetry.as_ref() {
                        request.attributes = attributes_with_telemetry(&request.attributes, collector);
                    }
                    insert_status(
                        &mut request.attributes,
                        report_context.report(&reporter.status(), heartbeat_seq, clock_skew_ms),
                    );
                    match replica_client.api
                        .heartbeat_replica(replica_client.replica_id(), &request)
                        .await
                    {
                        Ok(replica) => {
                            clock_skew_ms = replica
                                .last_seen_at
                                .signed_duration_since(Utc::now())
                                .num_milliseconds();
                        }
                        Err(err) => reporter.record_error(format!("heartbeat failed: {err}")),
                    }
                }
            }
        }
    })
}

async fn mark_offline(replica_client: &ReplicaClient<StaticLocator>, reporter: &StatusReporter) {
    if let Err(err) = replica_client
        .api
        .mark_replica_offline(
            replica_client.replica_id(),
            &replica_client.session.offline_request(),
        )
        .await
    {
        reporter.record_error(format!("failed to mark replica offline: {err}"));
    }
}

/// publish every provider this agent can run, so the service knows what to route here. best effort
/// per provider: one rejected entry must not keep the agent from starting its loop.
pub async fn publish_provider_metadata(
    replica_client: &ReplicaClient<StaticLocator>,
    config: &AgentRuntimeConfig,
    reporter: &StatusReporter,
) {
    let providers = (config.providers)();
    let total = providers.len();
    let mut published = 0usize;
    for provider in providers {
        match replica_client.register_provider(provider.metadata()).await {
            Ok(_) => published += 1,
            Err(err) => reporter.log(format!(
                "Failed to publish provider metadata for '{}': {err}",
                provider.name()
            )),
        }
    }
    reporter.log(format!(
        "Published provider metadata ({published} of {total})."
    ));
}

// the registration payload, including the routing facts the engine needs to target this agent.
fn replica_service_config(config: &AgentRuntimeConfig) -> ReplicaServiceConfig {
    ReplicaServiceConfig {
        replica_type: ReplicaKind::Worker,
        instance_id: config.instance_id.clone(),
        display_name: config.display_name.clone(),
        host: config.advertise_host.clone(),
        port: None,
        base_path: None,
        version: config.version.clone(),
        attributes: registration_attributes(config),
        heartbeat_interval: config.heartbeat_interval,
    }
}

// merge the routing facts every agent advertises into the host's own attributes, then stamp host
// metadata. keeping `labels`/`exclusive` here (rather than in each host) is what makes the two
// hosts' registrations comparable in the replica registry.
pub(super) fn registration_attributes(config: &AgentRuntimeConfig) -> Value {
    let mut attributes = match &config.attributes {
        Value::Object(_) => config.attributes.clone(),
        _ => Value::Object(Default::default()),
    };
    let mut labels = Map::default();
    for (key, value) in &config.labels {
        labels.insert(key.clone(), Value::String(value.clone()));
    }
    if let Some(object) = attributes.as_object_mut() {
        object.insert("labels".to_string(), Value::Object(labels));
        object.insert("exclusive".to_string(), Value::Bool(config.exclusive));
        object.insert(
            "broker_backend".to_string(),
            Value::String(config.broker.broker_backend.clone()),
        );
        object.insert(
            "broker_connection".to_string(),
            Value::String(config.broker_description.clone()),
        );
    }
    attributes_with_host_metadata(&attributes)
}

fn insert_status(attributes: &mut Value, status: runinator_models::replicas::AgentStatusReport) {
    if !attributes.is_object() {
        *attributes = Value::Object(Default::default());
    }
    if let Some(object) = attributes.as_object_mut() {
        let status = serde_json::to_value(status)
            .map(Value::from)
            .unwrap_or(Value::Null);
        object.insert("status".to_string(), status);
    }
}

#[cfg(test)]
#[path = "registration_tests.rs"]
mod tests;
