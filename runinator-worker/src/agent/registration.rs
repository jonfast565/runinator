//! Broker-mediated replica availability for the shared agent lifecycle.
//!
//! Workers announce the same durable lifecycle facts as every non-web-service runtime, but do so
//! through broker ingress. They therefore never need to call the web service merely to become
//! visible, routable, and live in the fleet view.

use std::{collections::BTreeMap, sync::Arc};

use chrono::Utc;
use runinator_broker::{Broker, IngressMessage};
use runinator_comm::WsIngressCommand;
use runinator_models::replicas::{ReplicaKind, ReplicaRegistrationRequest};
use runinator_models::value::{Map, Value};
use runinator_observability::resource_telemetry::{
    TelemetryCollector, attributes_with_host_metadata, attributes_with_telemetry,
};
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::agent::config::AgentRuntimeConfig;
use crate::agent::reporter::StatusReporter;
use crate::agent::shutdown::Shutdown;
use crate::agent::status::AgentReportContext;

/// Send the startup availability observation. A successful broker publish is required before the
/// worker starts consuming effects, so a process cannot silently execute as an untracked phantom.
pub async fn announce_agent_replica(
    broker: &dyn Broker,
    config: &AgentRuntimeConfig,
    reporter: &StatusReporter,
    report_context: &AgentReportContext,
    replica_id: Uuid,
    runtime_id: &str,
) -> Result<(), runinator_broker::BrokerError> {
    let availability = AgentAvailability::from_config(config);
    publish_agent_availability(
        broker,
        &availability,
        reporter,
        report_context,
        replica_id,
        runtime_id,
        0,
        None,
    )
    .await
}

/// Heartbeat the agent through broker ingress and explicitly retire it on a clean stop.
#[allow(
    clippy::too_many_arguments,
    reason = "the spawned task takes the distinct agent runtime resources it owns for its full lifetime"
)]
pub fn spawn_agent_heartbeat(
    broker: Arc<dyn Broker>,
    config: &AgentRuntimeConfig,
    replica_id: Uuid,
    runtime_id: String,
    reporter: Arc<StatusReporter>,
    report_context: Arc<AgentReportContext>,
    telemetry: Option<Arc<TelemetryCollector>>,
    shutdown: Shutdown,
) -> JoinHandle<()> {
    let availability = AgentAvailability::from_config(config);
    let heartbeat_interval = config.heartbeat_interval;
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(heartbeat_interval);
        let mut heartbeat_seq = 0u64;
        let shutdown_notify = shutdown.notify();
        loop {
            if shutdown.is_stopping() {
                mark_offline(broker.as_ref(), replica_id, &runtime_id, &reporter).await;
                return;
            }
            tokio::select! {
                _ = shutdown_notify.notified() => {
                    mark_offline(broker.as_ref(), replica_id, &runtime_id, &reporter).await;
                    return;
                }
                _ = ticker.tick() => {
                    heartbeat_seq = heartbeat_seq.saturating_add(1);
                    if let Err(err) = publish_agent_availability(
                        broker.as_ref(),
                        &availability,
                        reporter.as_ref(),
                        report_context.as_ref(),
                        replica_id,
                        &runtime_id,
                        heartbeat_seq,
                        telemetry.as_deref(),
                    ).await {
                        reporter.record_error(format!("availability heartbeat failed: {err}"));
                    }
                }
            }
        }
    })
}

async fn mark_offline(
    broker: &dyn Broker,
    replica_id: Uuid,
    runtime_id: &str,
    reporter: &StatusReporter,
) {
    let command = WsIngressCommand::replica_offline(replica_id, runtime_id);
    if let Err(err) = broker
        .publish_ingress(IngressMessage {
            dedupe_key: Some(command.dedupe_key()),
            command,
            enqueued_at: Utc::now(),
        })
        .await
    {
        reporter.record_error(format!("failed to mark replica offline: {err}"));
    }
}

#[derive(Clone)]
struct AgentAvailability {
    instance_id: String,
    display_name: Option<String>,
    host: Option<String>,
    version: Option<String>,
    attributes: Value,
    providers: crate::provider_repository::ProviderFactory,
    publish_providers: bool,
}

impl AgentAvailability {
    fn from_config(config: &AgentRuntimeConfig) -> Self {
        Self {
            instance_id: config.instance_id.clone(),
            display_name: config.display_name.clone(),
            host: config.advertise_host.clone(),
            version: config.version.clone(),
            attributes: registration_attributes(config),
            providers: Arc::clone(&config.providers),
            publish_providers: config.publish_providers,
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn publish_agent_availability(
    broker: &dyn Broker,
    availability: &AgentAvailability,
    reporter: &StatusReporter,
    report_context: &AgentReportContext,
    replica_id: Uuid,
    runtime_id: &str,
    heartbeat_seq: u64,
    telemetry: Option<&TelemetryCollector>,
) -> Result<(), runinator_broker::BrokerError> {
    let mut attributes = availability.attributes.clone();
    if let Some(telemetry) = telemetry {
        attributes = attributes_with_telemetry(&attributes, telemetry);
    }
    insert_status(
        &mut attributes,
        report_context.report(&reporter.status(), heartbeat_seq, 0),
    );
    let providers = if availability.publish_providers {
        {
            (availability.providers)()
                .into_iter()
                .map(|provider| provider.metadata())
                .collect()
        }
    } else {
        Default::default()
    };
    let command = WsIngressCommand::replica_available(
        ReplicaRegistrationRequest {
            replica_id: Some(replica_id),
            replica_type: ReplicaKind::Worker,
            instance_id: availability.instance_id.clone(),
            runtime_id: runtime_id.to_string(),
            display_name: availability.display_name.clone(),
            host: availability.host.clone(),
            port: None,
            base_path: None,
            version: availability.version.clone(),
            attributes,
        },
        providers,
    );
    broker
        .publish_ingress(IngressMessage {
            dedupe_key: Some(command.dedupe_key()),
            command,
            enqueued_at: Utc::now(),
        })
        .await
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
    for (key, value) in routing_labels(&config.labels, &config.instance_id) {
        labels.insert(key, Value::String(value));
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

pub(super) fn routing_labels(
    configured: &BTreeMap<String, String>,
    instance_id: &str,
) -> BTreeMap<String, String> {
    let mut labels = configured.clone();
    labels.insert(
        runinator_models::workspaces::WORKSPACE_INSTANCE_LABEL.to_string(),
        instance_id.to_string(),
    );
    labels
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
mod tests {
    use super::*;

    #[test]
    fn stable_instance_label_is_reserved_and_shared_by_routing_surfaces() {
        let configured = BTreeMap::from([
            ("pool".into(), "local".into()),
            (
                runinator_models::workspaces::WORKSPACE_INSTANCE_LABEL.into(),
                "caller-cannot-override".into(),
            ),
        ]);
        let labels = routing_labels(&configured, "worker-a");
        assert_eq!(labels.get("pool").map(String::as_str), Some("local"));
        assert_eq!(
            labels
                .get(runinator_models::workspaces::WORKSPACE_INSTANCE_LABEL)
                .map(String::as_str),
            Some("worker-a")
        );
    }
}
