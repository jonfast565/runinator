use std::sync::Arc;

use runinator_broker::{
    Broker, BrokerError,
    adapters::{kafka::KafkaBrokerConfig, rabbitmq::RabbitMqBrokerConfig},
    http::client::HttpBroker,
    in_memory::InMemoryBroker,
    tcp::client::TcpBroker,
    ws::client::WsBroker,
};
use runinator_models::errors::{RuntimeError, SendableError};

use crate::config;

/// the subset of worker config that selects and builds a `Broker`, factored out of the full CLI
/// [`config::Config`] so any caller (the standalone `runinator-worker` binary, or an embedded host
/// like `runinator-desktop-agent`) can pick a backend without needing to construct the rest of a
/// worker's CLI-oriented config. "which broker transport" and "what kind of worker this is" are
/// orthogonal: any worker — cloud or desktop — can connect directly to a broker backend
/// (tcp/rabbitmq/kafka/http/in-memory) or relay through `runinator-ws`'s `/ws/desktop-worker`
/// endpoint (`"ws"`) depending on what network access it actually has.
#[derive(Debug, Clone)]
pub struct BrokerConfig {
    pub broker_backend: String,
    pub broker_endpoint: String,
    pub broker_action_topic: String,
    pub broker_control_topic: String,
    pub broker_result_topic: String,
    pub broker_client_id: String,
    /// presented as a bearer token; only used by the `http`/`ws` backends today.
    pub api_key: Option<String>,
}

impl config::Config {
    /// the broker-relevant slice of this worker's full CLI config, for [`build_broker`].
    pub fn broker_config(&self) -> BrokerConfig {
        BrokerConfig {
            broker_backend: self.broker_backend.clone(),
            broker_endpoint: self.broker_endpoint.clone(),
            broker_action_topic: self.broker_action_topic.clone(),
            broker_control_topic: self.broker_control_topic.clone(),
            broker_result_topic: self.broker_result_topic.clone(),
            broker_client_id: self.broker_client_id.clone(),
            api_key: self.api_key.clone(),
        }
    }
}

pub async fn build_broker(config: &BrokerConfig) -> Result<Arc<dyn Broker>, SendableError> {
    runinator_broker::ensure_named_workflow_result_channel(
        &config.broker_backend,
        &config.broker_result_topic,
    )
    .map_err(|err| broker_error("workflow_results", err))?;

    let broker: Arc<dyn Broker> = match config.broker_backend.as_str() {
        "http" => {
            let url = reqwest::Url::parse(&config.broker_endpoint)
                .map_err(|err| crate::errors::BROKER_INVALID_ENDPOINT.error(err))?;

            let client = reqwest::Client::builder()
                .build()
                .map_err(|err| crate::errors::BROKER_CLIENT.error(err))?;

            Arc::new(HttpBroker::new(url, client))
        }
        "ws" => Arc::new(WsBroker::connect(
            config.broker_endpoint.clone(),
            config.api_key.clone(),
        )),
        "in-memory" => Arc::new(InMemoryBroker::new()),
        "tcp" => Arc::new(TcpBroker::new(config.broker_endpoint.clone())),
        "kafka" => runinator_broker::build_kafka_broker(
            KafkaBrokerConfig::new(config.broker_endpoint.clone())
                .with_topics(
                    config.broker_action_topic.clone(),
                    config.broker_control_topic.clone(),
                    config.broker_result_topic.clone(),
                )
                .with_client_id(config.broker_client_id.clone()),
        )
        .map_err(|err| crate::errors::BROKER_KAFKA.error(err))?,
        "rabbitmq" => runinator_broker::build_rabbitmq_broker(
            RabbitMqBrokerConfig::new(config.broker_endpoint.clone())
                .with_queues(
                    config.broker_action_topic.clone(),
                    config.broker_control_topic.clone(),
                    config.broker_result_topic.clone(),
                )
                .with_client_id(config.broker_client_id.clone()),
        )
        .await
        .map_err(|err| crate::errors::BROKER_RABBITMQ.error(err))?,
        other => {
            return Err(crate::errors::BROKER_UNKNOWN_BACKEND.error(format!("'{other}'")));
        }
    };

    runinator_broker::ensure_workflow_result_channels_supported(
        &config.broker_backend,
        broker.as_ref(),
    )
    .map_err(|err| broker_error("workflow_results", err))?;

    // wrap the concrete backend so every broker operation emits otel metrics tagged with the backend.
    Ok(runinator_broker::instrument(
        broker,
        config.broker_backend.clone(),
    ))
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
