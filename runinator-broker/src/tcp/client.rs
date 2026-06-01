use crate::{
    tcp::types::{TcpRequest, TcpResponse},
    Broker, BrokerDelivery, BrokerError, BrokerMessage, ControlCommand, ControlDelivery,
    IngressDelivery, IngressMessage, ResultDelivery, ResultMessage, WakeDelivery, WakeMessage,
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
    fn supports_workflow_result_channels(&self) -> bool {
        true
    }

    async fn publish(&self, message: BrokerMessage) -> Result<(), BrokerError> {
        let response = self.request(TcpRequest::Publish { message }).await?;
        Self::expect_ok(response)
    }

    async fn receive(&self, consumer: &str) -> Result<BrokerDelivery, BrokerError> {
        match self
            .receive_request(TcpRequest::Receive {
                consumer: consumer.to_string(),
            })
            .await?
        {
            TcpResponse::Delivery { delivery } => Ok(delivery),
            TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
            _ => Err(BrokerError::Internal(
                "unexpected action delivery response".into(),
            )),
        }
    }

    async fn ack(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        let response = self
            .request(TcpRequest::Ack {
                consumer: consumer.to_string(),
                delivery_id,
            })
            .await?;
        Self::expect_ok(response)
    }

    async fn nack(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        let response = self
            .request(TcpRequest::Nack {
                consumer: consumer.to_string(),
                delivery_id,
            })
            .await?;
        Self::expect_ok(response)
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

    async fn ack_control(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        let response = self
            .request(TcpRequest::AckControl {
                consumer: consumer.to_string(),
                delivery_id,
            })
            .await?;
        Self::expect_ok(response)
    }

    async fn publish_result(&self, message: ResultMessage) -> Result<(), BrokerError> {
        let response = self.request(TcpRequest::PublishResult { message }).await?;
        Self::expect_ok(response)
    }

    async fn receive_result(&self, consumer: &str) -> Result<ResultDelivery, BrokerError> {
        match self
            .receive_request(TcpRequest::ReceiveResult {
                consumer: consumer.to_string(),
            })
            .await?
        {
            TcpResponse::ResultDelivery { delivery } => Ok(delivery),
            TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
            _ => Err(BrokerError::Internal(
                "unexpected result delivery response".into(),
            )),
        }
    }

    async fn ack_result(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        let response = self
            .request(TcpRequest::AckResult {
                consumer: consumer.to_string(),
                delivery_id,
            })
            .await?;
        Self::expect_ok(response)
    }

    async fn nack_result(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        let response = self
            .request(TcpRequest::NackResult {
                consumer: consumer.to_string(),
                delivery_id,
            })
            .await?;
        Self::expect_ok(response)
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
