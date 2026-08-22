use crate::{
    tcp::types::{TcpRequest, TcpResponse},
    AgentCommand, AgentDelivery, Broker, BrokerError, ConsumerProfile, ControlCommand,
    ControlDelivery, EffectDelivery, EffectMessage, EffectResultDelivery, EffectResultMessage,
    EventDelivery, EventMessage, IngressDelivery, IngressMessage, WakeDelivery, WakeMessage,
};
use async_trait::async_trait;
use std::time::Duration;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
    time,
};
use uuid::Uuid;

#[derive(Clone)]
pub struct TcpBroker {
    endpoint: String,
    request_timeout: Duration,
}

impl TcpBroker {
    const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

    pub fn new(endpoint: impl Into<String>) -> Self {
        Self::with_timeout(endpoint, Self::DEFAULT_REQUEST_TIMEOUT)
    }

    pub fn with_timeout(endpoint: impl Into<String>, request_timeout: Duration) -> Self {
        Self {
            endpoint: endpoint.into(),
            request_timeout,
        }
    }

    async fn request(&self, request: TcpRequest) -> Result<TcpResponse, BrokerError> {
        self.request_inner(request, true).await
    }

    async fn receive_request(&self, request: TcpRequest) -> Result<TcpResponse, BrokerError> {
        self.request_inner(request, false).await
    }

    async fn request_inner(
        &self,
        request: TcpRequest,
        timeout_response: bool,
    ) -> Result<TcpResponse, BrokerError> {
        let mut stream = timeout_io(
            self.request_timeout,
            "connect",
            TcpStream::connect(self.endpoint.as_str()),
        )
        .await?;
        let payload = serde_json::to_string(&request)
            .map_err(|err| BrokerError::Internal(err.to_string()))?;
        timeout_io(
            self.request_timeout,
            "write",
            stream.write_all(payload.as_bytes()),
        )
        .await?;
        timeout_io(self.request_timeout, "write", stream.write_all(b"\n")).await?;

        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        if timeout_response {
            timeout_io(self.request_timeout, "read", reader.read_line(&mut line)).await?;
        } else {
            reader
                .read_line(&mut line)
                .await
                .map_err(|err| BrokerError::Internal(err.to_string()))?;
        }
        if line.is_empty() {
            return Err(BrokerError::Internal("broker closed connection".into()));
        }
        serde_json::from_str(line.trim_end()).map_err(|err| BrokerError::Internal(err.to_string()))
    }

    fn expect_ok(response: TcpResponse) -> Result<(), BrokerError> {
        match response {
            TcpResponse::Ok => Ok(()),
            TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
            _ => Err(BrokerError::Internal("unexpected delivery response".into())),
        }
    }
}

#[async_trait]
impl Broker for TcpBroker {
    fn supports_workflow_effect_channels(&self) -> bool {
        true
    }

    fn supports_agent_channel(&self) -> bool {
        true
    }

    async fn heartbeat(&self) -> Result<(), BrokerError> {
        Self::expect_ok(self.request(TcpRequest::Heartbeat).await?)
    }

    async fn publish_control(&self, command: ControlCommand) -> Result<(), BrokerError> {
        let response = self.request(TcpRequest::PublishControl { command }).await?;
        Self::expect_ok(response)
    }

    async fn receive_control(&self, consumer: &str) -> Result<ControlDelivery, BrokerError> {
        match self
            .receive_request(TcpRequest::ReceiveControl {
                consumer: consumer.to_string(),
            })
            .await?
        {
            TcpResponse::ControlDelivery { delivery } => Ok(delivery),
            TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
            _ => Err(BrokerError::Internal(
                "unexpected control delivery response".into(),
            )),
        }
    }

    async fn receive_control_for(
        &self,
        profile: &ConsumerProfile,
    ) -> Result<ControlDelivery, BrokerError> {
        match self
            .receive_request(TcpRequest::ReceiveControlFor {
                profile: profile.clone(),
            })
            .await?
        {
            TcpResponse::ControlDelivery { delivery } => Ok(delivery),
            TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
            _ => Err(BrokerError::Internal(
                "unexpected control delivery response".into(),
            )),
        }
    }

    async fn ack_control(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        let response = self
            .request(TcpRequest::AckControl {
                consumer: consumer.to_string(),
                delivery_id,
            })
            .await?;
        Self::expect_ok(response)
    }

    async fn nack_control(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        let response = self
            .request(TcpRequest::NackControl {
                consumer: consumer.to_string(),
                delivery_id,
            })
            .await?;
        Self::expect_ok(response)
    }

    async fn publish_agent(&self, command: AgentCommand) -> Result<(), BrokerError> {
        let response = self.request(TcpRequest::PublishAgent { command }).await?;
        Self::expect_ok(response)
    }

    async fn receive_agent(&self, consumer: &str) -> Result<AgentDelivery, BrokerError> {
        match self
            .receive_request(TcpRequest::ReceiveAgent {
                consumer: consumer.to_string(),
            })
            .await?
        {
            TcpResponse::AgentDelivery { delivery } => Ok(delivery),
            TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
            _ => Err(BrokerError::Internal(
                "unexpected agent delivery response".into(),
            )),
        }
    }

    async fn receive_agent_for(
        &self,
        profile: &ConsumerProfile,
    ) -> Result<AgentDelivery, BrokerError> {
        match self
            .receive_request(TcpRequest::ReceiveAgentFor {
                profile: profile.clone(),
            })
            .await?
        {
            TcpResponse::AgentDelivery { delivery } => Ok(delivery),
            TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
            _ => Err(BrokerError::Internal(
                "unexpected agent delivery response".into(),
            )),
        }
    }

    async fn ack_agent(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        let response = self
            .request(TcpRequest::AckAgent {
                consumer: consumer.to_string(),
                delivery_id,
            })
            .await?;
        Self::expect_ok(response)
    }

    async fn nack_agent(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        let response = self
            .request(TcpRequest::NackAgent {
                consumer: consumer.to_string(),
                delivery_id,
            })
            .await?;
        Self::expect_ok(response)
    }

    async fn publish_effect(&self, message: EffectMessage) -> Result<(), BrokerError> {
        Self::expect_ok(self.request(TcpRequest::PublishEffect { message }).await?)
    }

    async fn receive_effect(&self, consumer: &str) -> Result<EffectDelivery, BrokerError> {
        match self
            .receive_request(TcpRequest::ReceiveEffect {
                consumer: consumer.to_string(),
            })
            .await?
        {
            TcpResponse::EffectDelivery { delivery } => Ok(delivery),
            TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
            _ => Err(BrokerError::Internal(
                "unexpected effect delivery response".into(),
            )),
        }
    }

    async fn receive_effect_for(
        &self,
        profile: &ConsumerProfile,
    ) -> Result<EffectDelivery, BrokerError> {
        match self
            .receive_request(TcpRequest::ReceiveEffectFor {
                profile: profile.clone(),
            })
            .await?
        {
            TcpResponse::EffectDelivery { delivery } => Ok(delivery),
            TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
            _ => Err(BrokerError::Internal(
                "unexpected effect delivery response".into(),
            )),
        }
    }

    async fn receive_infrastructure_effect(
        &self,
        consumer: &str,
    ) -> Result<EffectDelivery, BrokerError> {
        match self
            .receive_request(TcpRequest::ReceiveInfrastructureEffect {
                consumer: consumer.to_string(),
            })
            .await?
        {
            TcpResponse::EffectDelivery { delivery } => Ok(delivery),
            TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
            _ => Err(BrokerError::Internal(
                "unexpected infrastructure-effect delivery response".into(),
            )),
        }
    }

    async fn ack_effect(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        Self::expect_ok(
            self.request(TcpRequest::AckEffect {
                consumer: consumer.to_string(),
                delivery_id,
            })
            .await?,
        )
    }

    async fn nack_effect(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        Self::expect_ok(
            self.request(TcpRequest::NackEffect {
                consumer: consumer.to_string(),
                delivery_id,
            })
            .await?,
        )
    }

    async fn publish_effect_result(&self, message: EffectResultMessage) -> Result<(), BrokerError> {
        Self::expect_ok(
            self.request(TcpRequest::PublishEffectResult { message })
                .await?,
        )
    }

    async fn receive_effect_result(
        &self,
        consumer: &str,
    ) -> Result<EffectResultDelivery, BrokerError> {
        match self
            .receive_request(TcpRequest::ReceiveEffectResult {
                consumer: consumer.to_string(),
            })
            .await?
        {
            TcpResponse::EffectResultDelivery { delivery } => Ok(delivery),
            TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
            _ => Err(BrokerError::Internal(
                "unexpected effect-result delivery response".into(),
            )),
        }
    }

    async fn ack_effect_result(
        &self,
        consumer: &str,
        delivery_id: Uuid,
    ) -> Result<(), BrokerError> {
        Self::expect_ok(
            self.request(TcpRequest::AckEffectResult {
                consumer: consumer.to_string(),
                delivery_id,
            })
            .await?,
        )
    }

    async fn nack_effect_result(
        &self,
        consumer: &str,
        delivery_id: Uuid,
    ) -> Result<(), BrokerError> {
        Self::expect_ok(
            self.request(TcpRequest::NackEffectResult {
                consumer: consumer.to_string(),
                delivery_id,
            })
            .await?,
        )
    }

    async fn publish_wake(&self, message: WakeMessage) -> Result<(), BrokerError> {
        let response = self.request(TcpRequest::PublishWake { message }).await?;
        Self::expect_ok(response)
    }

    async fn receive_wake(&self, consumer: &str) -> Result<WakeDelivery, BrokerError> {
        match self
            .receive_request(TcpRequest::ReceiveWake {
                consumer: consumer.to_string(),
            })
            .await?
        {
            TcpResponse::WakeDelivery { delivery } => Ok(delivery),
            TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
            _ => Err(BrokerError::Internal(
                "unexpected wake delivery response".into(),
            )),
        }
    }

    async fn ack_wake(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        let response = self
            .request(TcpRequest::AckWake {
                consumer: consumer.to_string(),
                delivery_id,
            })
            .await?;
        Self::expect_ok(response)
    }

    async fn nack_wake(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        let response = self
            .request(TcpRequest::NackWake {
                consumer: consumer.to_string(),
                delivery_id,
            })
            .await?;
        Self::expect_ok(response)
    }

    async fn publish_ingress(&self, message: IngressMessage) -> Result<(), BrokerError> {
        let response = self.request(TcpRequest::PublishIngress { message }).await?;
        Self::expect_ok(response)
    }

    async fn receive_ingress(&self, consumer: &str) -> Result<IngressDelivery, BrokerError> {
        match self
            .receive_request(TcpRequest::ReceiveIngress {
                consumer: consumer.to_string(),
            })
            .await?
        {
            TcpResponse::IngressDelivery { delivery } => Ok(delivery),
            TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
            _ => Err(BrokerError::Internal(
                "unexpected ingress delivery response".into(),
            )),
        }
    }

    async fn ack_ingress(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        let response = self
            .request(TcpRequest::AckIngress {
                consumer: consumer.to_string(),
                delivery_id,
            })
            .await?;
        Self::expect_ok(response)
    }

    async fn nack_ingress(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        let response = self
            .request(TcpRequest::NackIngress {
                consumer: consumer.to_string(),
                delivery_id,
            })
            .await?;
        Self::expect_ok(response)
    }

    async fn publish_event(&self, message: EventMessage) -> Result<(), BrokerError> {
        let response = self.request(TcpRequest::PublishEvent { message }).await?;
        Self::expect_ok(response)
    }

    async fn receive_event(&self, consumer: &str) -> Result<EventDelivery, BrokerError> {
        match self
            .receive_request(TcpRequest::ReceiveEvent {
                consumer: consumer.to_string(),
            })
            .await?
        {
            TcpResponse::EventDelivery { delivery } => Ok(delivery),
            TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
            _ => Err(BrokerError::Internal(
                "unexpected event delivery response".into(),
            )),
        }
    }
}

async fn timeout_io<T, F>(
    duration: Duration,
    operation: &'static str,
    future: F,
) -> Result<T, BrokerError>
where
    F: std::future::Future<Output = std::io::Result<T>>,
{
    match time::timeout(duration, future).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(err)) => Err(BrokerError::Internal(err.to_string())),
        Err(_) => Err(BrokerError::Internal(format!(
            "tcp broker {operation} timed out after {} ms",
            duration.as_millis()
        ))),
    }
}
