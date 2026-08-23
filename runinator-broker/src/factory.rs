use std::sync::Arc;
use std::{error::Error, fmt};

use crate::{
    adapters::{kafka::KafkaBrokerConfig, rabbitmq::RabbitMqBrokerConfig},
    Broker, BrokerError,
};

/// Transport-independent settings used by long-running Runinator processes.
///
/// The channel fields are only consumed by direct Kafka/RabbitMQ connections; the wire
/// transports intentionally ignore them. `wake_topic` and `ingress_topic` opt a client into the
/// orchestration channels used by the waker.
#[derive(Debug, Clone)]
pub struct BrokerClientConfig {
    pub backend: String,
    pub endpoint: String,
    pub control_topic: String,
    pub agent_topic: Option<String>,
    pub effect_topic: String,
    pub infrastructure_effect_topic: String,
    pub effect_result_topic: String,
    pub client_id: String,
    pub relay_credential: Option<String>,
    pub wake_topic: Option<String>,
    pub ingress_topic: Option<String>,
}

/// The channels a caller needs from a broker connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrokerConsumerProfile {
    /// A workflow runtime publishes effect commands and consumes their results.
    WorkflowRuntime,
    /// The timer relay only needs the wake and ingress channels.
    Waker,
    /// A broker-only service that announces lifecycle observations on ingress but consumes no
    /// workflow channel (the archiver today).
    IngressPublisher,
    /// A provider worker consumes effects/control and publishes effect results.
    Worker,
}

impl BrokerConsumerProfile {
    fn needs_workflow_effect_channels(self) -> bool {
        matches!(self, Self::WorkflowRuntime | Self::Worker)
    }
}

/// Startup errors produced before a concrete broker is available.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrokerBuildError {
    InvalidEndpoint { endpoint: String, message: String },
    UnknownBackend(String),
    Backend { backend: String, message: String },
    Capability { backend: String, message: String },
}

impl fmt::Display for BrokerBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidEndpoint { endpoint, message } => {
                write!(f, "invalid broker endpoint '{endpoint}': {message}")
            }
            Self::UnknownBackend(backend) => write!(f, "unknown broker backend '{backend}'"),
            Self::Backend { backend, message } => {
                write!(f, "broker backend '{backend}': {message}")
            }
            Self::Capability { backend, message } => {
                write!(
                    f,
                    "broker backend '{backend}' does not support required channels: {message}"
                )
            }
        }
    }
}

impl Error for BrokerBuildError {}

/// Construct, validate, and instrument a broker client for a long-running process.
pub async fn build_broker_client(
    config: &BrokerClientConfig,
    profile: BrokerConsumerProfile,
) -> Result<Arc<dyn Broker>, BrokerBuildError> {
    let effect_channel = match config.backend.as_str() {
        "kafka" | "rabbitmq" => config.effect_topic.as_str(),
        _ => "",
    };
    if profile.needs_workflow_effect_channels() {
        runinator_broker_core::ensure_named_workflow_effect_channel(
            &config.backend,
            effect_channel,
        )
        .map_err(|err| BrokerBuildError::Capability {
            backend: config.backend.clone(),
            message: err.to_string(),
        })?;
    }

    let broker: Arc<dyn Broker> = match config.backend.as_str() {
        #[cfg(feature = "http")]
        "http" => {
            let url = reqwest::Url::parse(&config.endpoint).map_err(|err| {
                BrokerBuildError::InvalidEndpoint {
                    endpoint: config.endpoint.clone(),
                    message: err.to_string(),
                }
            })?;
            let client =
                reqwest::Client::builder()
                    .build()
                    .map_err(|err| BrokerBuildError::Backend {
                        backend: config.backend.clone(),
                        message: err.to_string(),
                    })?;
            Arc::new(crate::http::client::HttpBroker::new(url, client))
        }
        #[cfg(feature = "ws")]
        "ws" => Arc::new(crate::ws::client::WsBroker::connect(
            config.endpoint.clone(),
            config.relay_credential.clone(),
        )),
        #[cfg(not(feature = "ws"))]
        "ws" => {
            return Err(BrokerBuildError::Backend {
                backend: "ws".into(),
                message: "feature is not enabled".into(),
            });
        }
        "in-memory" => Arc::new(runinator_broker_core::in_memory::InMemoryBroker::new()),
        #[cfg(feature = "tcp")]
        "tcp" => Arc::new(crate::tcp::client::TcpBroker::new(config.endpoint.clone())),
        "kafka" => {
            build_kafka_broker(kafka_config(config)).map_err(|err| BrokerBuildError::Backend {
                backend: config.backend.clone(),
                message: err.to_string(),
            })?
        }
        "rabbitmq" => build_rabbitmq_broker(rabbitmq_config(config))
            .await
            .map_err(|err| BrokerBuildError::Backend {
                backend: config.backend.clone(),
                message: err.to_string(),
            })?,
        other => return Err(BrokerBuildError::UnknownBackend(other.to_string())),
    };

    if profile.needs_workflow_effect_channels() {
        runinator_broker_core::ensure_workflow_effect_channels_supported(
            &config.backend,
            broker.as_ref(),
        )
        .map_err(|err| BrokerBuildError::Capability {
            backend: config.backend.clone(),
            message: err.to_string(),
        })?;
    }
    Ok(runinator_broker_core::instrument(
        broker,
        config.backend.clone(),
    ))
}

fn kafka_config(config: &BrokerClientConfig) -> KafkaBrokerConfig {
    let base = KafkaBrokerConfig::new(config.endpoint.clone())
        .with_control_topic(config.control_topic.clone())
        .with_effect_topics(
            config.effect_topic.clone(),
            config.infrastructure_effect_topic.clone(),
            config.effect_result_topic.clone(),
        )
        .with_client_id(config.client_id.clone());
    let base = match &config.agent_topic {
        Some(agent) => base.with_agent_topic(agent.clone()),
        None => base,
    };
    let base = match &config.wake_topic {
        Some(wake) => base.with_wake_topic(wake.clone()),
        None => base,
    };
    match &config.ingress_topic {
        Some(ingress) => base.with_ingress_topic(ingress.clone()),
        None => base,
    }
}

fn rabbitmq_config(config: &BrokerClientConfig) -> RabbitMqBrokerConfig {
    let base = RabbitMqBrokerConfig::new(config.endpoint.clone())
        .with_control_queue(config.control_topic.clone())
        .with_effect_queues(
            config.effect_topic.clone(),
            config.infrastructure_effect_topic.clone(),
            config.effect_result_topic.clone(),
        )
        .with_client_id(config.client_id.clone());
    let base = match &config.agent_topic {
        Some(agent) => base.with_agent_queue_prefix(agent.clone()),
        None => base,
    };
    let base = match &config.wake_topic {
        Some(wake) => base.with_wake_queue(wake.clone()),
        None => base,
    };
    match &config.ingress_topic {
        Some(ingress) => base.with_ingress_queue(ingress.clone()),
        None => base,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(backend: &str) -> BrokerClientConfig {
        BrokerClientConfig {
            backend: backend.into(),
            endpoint: "127.0.0.1:7070".into(),
            control_topic: "control".into(),
            agent_topic: Some("agent".into()),
            effect_topic: "effects".into(),
            infrastructure_effect_topic: "effects.infrastructure".into(),
            effect_result_topic: "effect-results".into(),
            client_id: "test".into(),
            relay_credential: Some("token".into()),
            wake_topic: Some("wake".into()),
            ingress_topic: Some("ingress".into()),
        }
    }

    #[test]
    fn direct_adapter_configs_preserve_all_channel_names() {
        let config = config("kafka");
        let kafka = kafka_config(&config);
        assert_eq!(kafka.effect_topic, "effects");
        assert_eq!(kafka.infrastructure_effect_topic, "effects.infrastructure");
        assert_eq!(kafka.effect_result_topic, "effect-results");
        assert_eq!(kafka.agent_topic, "agent");
        assert_eq!(kafka.wake_topic, "wake");
        assert_eq!(kafka.ingress_topic, "ingress");
        let rabbit = rabbitmq_config(&config);
        assert_eq!(rabbit.effect_queue, "effects");
        assert_eq!(rabbit.effect_result_queue, "effect-results");
        assert_eq!(rabbit.agent_queue_prefix, "agent");
        assert_eq!(rabbit.wake_queue, "wake");
        assert_eq!(rabbit.ingress_queue, "ingress");
    }

    #[test]
    fn direct_adapter_configs_allow_an_ingress_only_override() {
        let mut config = config("kafka");
        config.wake_topic = None;

        assert_eq!(kafka_config(&config).ingress_topic, "ingress");
        assert_eq!(rabbitmq_config(&config).ingress_queue, "ingress");
    }

    #[tokio::test]
    async fn in_memory_build_is_instrumented_for_each_profile() {
        let config = config("in-memory");
        for profile in [
            BrokerConsumerProfile::WorkflowRuntime,
            BrokerConsumerProfile::Waker,
            BrokerConsumerProfile::IngressPublisher,
            BrokerConsumerProfile::Worker,
        ] {
            assert!(build_broker_client(&config, profile).await.is_ok());
        }
    }

    #[tokio::test]
    async fn http_reports_invalid_endpoint() {
        let mut config = config("http");
        config.endpoint = "not a url".into();
        assert!(matches!(
            build_broker_client(&config, BrokerConsumerProfile::Worker).await,
            Err(BrokerBuildError::InvalidEndpoint { .. })
        ));
    }

    #[tokio::test]
    async fn workflow_profile_rejects_blank_direct_effect_channel_before_connecting() {
        let mut config = config("kafka");
        config.effect_topic = " ".into();
        assert!(matches!(
            build_broker_client(&config, BrokerConsumerProfile::WorkflowRuntime).await,
            Err(BrokerBuildError::Capability { .. })
        ));
    }
}

// construct a kafka-backed broker, or fail when the kafka feature is disabled.
#[cfg(feature = "kafka")]
pub fn build_kafka_broker(config: KafkaBrokerConfig) -> Result<Arc<dyn Broker>, BrokerError> {
    Ok(Arc::new(crate::adapters::kafka::KafkaBroker::new(config)?))
}

#[cfg(not(feature = "kafka"))]
pub fn build_kafka_broker(_config: KafkaBrokerConfig) -> Result<Arc<dyn Broker>, BrokerError> {
    Err(BrokerError::FeatureDisabled("kafka"))
}

// construct a rabbitmq-backed broker, or fail when the rabbitmq feature is disabled.
#[cfg(feature = "rabbitmq")]
pub async fn build_rabbitmq_broker(
    config: RabbitMqBrokerConfig,
) -> Result<Arc<dyn Broker>, BrokerError> {
    Ok(Arc::new(
        crate::adapters::rabbitmq::RabbitMqBroker::connect(config).await?,
    ))
}

#[cfg(not(feature = "rabbitmq"))]
pub async fn build_rabbitmq_broker(
    _config: RabbitMqBrokerConfig,
) -> Result<Arc<dyn Broker>, BrokerError> {
    Err(BrokerError::FeatureDisabled("rabbitmq"))
}
