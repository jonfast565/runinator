use std::sync::Arc;

use runinator_broker::{
    Broker, BrokerBuildError, BrokerClientConfig, BrokerConsumerProfile, BrokerError,
    build_broker_client,
};
use runinator_models::errors::{RuntimeError, SendableError};

use crate::config;

/// the subset of worker config that selects and builds a `Broker`, factored out of the full CLI
/// [`config::Config`] so any caller (the standalone `runinator-worker` binary, or an embedded host
/// like `runinator-desktop-agent`) can pick a backend without needing to construct the rest of a
/// worker's CLI-oriented config. "which broker transport" and "what kind of worker this is" are
/// orthogonal: any worker — cloud or desktop — can connect directly to a broker backend
/// (tcp/rabbitmq/kafka/http/in-memory) or relay through `runinator-ws`'s `/ws/desktop-worker`
/// endpoint (`"ws"`) depending on the available network path.
#[derive(Debug, Clone)]
pub struct BrokerConfig {
    pub broker_backend: String,
    pub broker_endpoint: String,
    pub broker_control_topic: String,
    pub broker_agent_topic: String,
    pub broker_effect_topic: String,
    pub broker_infrastructure_effect_topic: String,
    pub broker_effect_result_topic: String,
    pub broker_client_id: String,
    /// Presented as a bearer token. Only the `http` and `ws` backends use it today.
    pub api_key: Option<String>,
}

impl config::Config {
    /// the broker-relevant slice of this worker's full CLI config, for [`build_broker`].
    pub fn broker_config(&self) -> BrokerConfig {
        BrokerConfig {
            broker_backend: self.broker_backend.clone(),
            broker_endpoint: self.broker_endpoint.clone(),
            broker_control_topic: self.broker_control_topic.clone(),
            broker_agent_topic: self.broker_agent_topic.clone(),
            broker_effect_topic: self.broker_effect_topic.clone(),
            broker_infrastructure_effect_topic: self.broker_infrastructure_effect_topic.clone(),
            broker_effect_result_topic: self.broker_effect_result_topic.clone(),
            broker_client_id: self.broker_client_id.clone(),
            api_key: self.api_key.clone(),
        }
    }
}

pub async fn build_broker(config: &BrokerConfig) -> Result<Arc<dyn Broker>, SendableError> {
    build_broker_client(
        &BrokerClientConfig {
            backend: config.broker_backend.clone(),
            endpoint: config.broker_endpoint.clone(),
            control_topic: config.broker_control_topic.clone(),
            agent_topic: Some(config.broker_agent_topic.clone()),
            effect_topic: config.broker_effect_topic.clone(),
            infrastructure_effect_topic: config.broker_infrastructure_effect_topic.clone(),
            effect_result_topic: config.broker_effect_result_topic.clone(),
            client_id: config.broker_client_id.clone(),
            relay_credential: config.api_key.clone(),
            wake_topic: None,
            ingress_topic: None,
        },
        BrokerConsumerProfile::Worker,
    )
    .await
    .map_err(map_build_error)
}

fn map_build_error(err: BrokerBuildError) -> SendableError {
    match &err {
        BrokerBuildError::InvalidEndpoint { message, .. } => {
            crate::errors::BROKER_INVALID_ENDPOINT.error(message)
        }
        BrokerBuildError::UnknownBackend(backend) => {
            crate::errors::BROKER_UNKNOWN_BACKEND.error(backend)
        }
        BrokerBuildError::Backend { backend, message } if backend == "kafka" => {
            crate::errors::BROKER_KAFKA.error(message)
        }
        BrokerBuildError::Backend { backend, message } if backend == "rabbitmq" => {
            crate::errors::BROKER_RABBITMQ.error(message)
        }
        BrokerBuildError::Backend { message, .. } if message.contains("feature is not enabled") => {
            crate::errors::BROKER_FEATURE_DISABLED.error(message)
        }
        BrokerBuildError::Capability { message, .. } => broker_error(
            "workflow_results",
            BrokerError::WorkflowEffectsUnsupported(message.clone()),
        ),
        _ => Box::new(err),
    }
}

pub(crate) fn broker_error(context: &'static str, err: BrokerError) -> SendableError {
    // keep the per-context dotted key for back-compat while rendering the numbered code.
    let descriptor = crate::errors::BROKER_OPERATION;
    Box::new(RuntimeError::new(
        format!("worker.broker.{context}"),
        format!(
            "{} - {}: {context}: {err}",
            descriptor.code, descriptor.summary
        ),
    ))
}
