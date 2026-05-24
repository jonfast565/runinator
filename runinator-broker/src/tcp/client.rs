use crate::{
    tcp::types::{TcpRequest, TcpResponse},
    Broker, BrokerDelivery, BrokerError, BrokerMessage, ControlCommand, ControlDelivery,
    ResultDelivery, ResultMessage,
};
use async_trait::async_trait;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
};
use uuid::Uuid;

#[derive(Clone)]
pub struct TcpBroker {
    endpoint: String,
}

impl TcpBroker {
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
        }
    }

    async fn request(&self, request: TcpRequest) -> Result<TcpResponse, BrokerError> {
        let mut stream = TcpStream::connect(self.endpoint.as_str())
            .await
            .map_err(|err| BrokerError::Internal(err.to_string()))?;
        let payload = serde_json::to_string(&request)
            .map_err(|err| BrokerError::Internal(err.to_string()))?;
        stream
            .write_all(payload.as_bytes())
            .await
            .map_err(|err| BrokerError::Internal(err.to_string()))?;
        stream
            .write_all(b"\n")
            .await
            .map_err(|err| BrokerError::Internal(err.to_string()))?;

        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .await
            .map_err(|err| BrokerError::Internal(err.to_string()))?;
        if line.is_empty() {
            return Err(BrokerError::Internal("broker closed connection".into()));
        }
        serde_json::from_str(line.trim_end()).map_err(|err| BrokerError::Internal(err.to_string()))
    }

    fn expect_ok(response: TcpResponse) -> Result<(), BrokerError> {
        match response {
            TcpResponse::Ok => Ok(()),
            TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
            TcpResponse::Delivery { .. } => {
                Err(BrokerError::Internal("unexpected delivery response".into()))
            }
            TcpResponse::ControlDelivery { .. } => Err(BrokerError::Internal(
                "unexpected control delivery response".into(),
            )),
            TcpResponse::ResultDelivery { .. } => Err(BrokerError::Internal(
                "unexpected result delivery response".into(),
            )),
        }
    }
}

#[async_trait]
impl Broker for TcpBroker {
    async fn publish(&self, message: BrokerMessage) -> Result<(), BrokerError> {
        let response = self.request(TcpRequest::Publish { message }).await?;
        Self::expect_ok(response)
    }

    async fn receive(&self, consumer: &str) -> Result<BrokerDelivery, BrokerError> {
        match self
            .request(TcpRequest::Receive {
                consumer: consumer.to_string(),
            })
            .await?
        {
            TcpResponse::Delivery { delivery } => Ok(delivery),
            TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
            TcpResponse::Ok => Err(BrokerError::Internal("unexpected ok response".into())),
            TcpResponse::ControlDelivery { .. } => Err(BrokerError::Internal(
                "unexpected control delivery response".into(),
            )),
            TcpResponse::ResultDelivery { .. } => Err(BrokerError::Internal(
                "unexpected result delivery response".into(),
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
            .request(TcpRequest::ReceiveControl {
                consumer: consumer.to_string(),
            })
            .await?
        {
            TcpResponse::ControlDelivery { delivery } => Ok(delivery),
            TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
            TcpResponse::Ok => Err(BrokerError::Internal("unexpected ok response".into())),
            TcpResponse::Delivery { .. } => Err(BrokerError::Internal(
                "unexpected action delivery response".into(),
            )),
            TcpResponse::ResultDelivery { .. } => Err(BrokerError::Internal(
                "unexpected result delivery response".into(),
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
            .request(TcpRequest::ReceiveResult {
                consumer: consumer.to_string(),
            })
            .await?
        {
            TcpResponse::ResultDelivery { delivery } => Ok(delivery),
            TcpResponse::Error { message } => Err(BrokerError::Internal(message)),
            TcpResponse::Ok => Err(BrokerError::Internal("unexpected ok response".into())),
            TcpResponse::Delivery { .. } => Err(BrokerError::Internal(
                "unexpected action delivery response".into(),
            )),
            TcpResponse::ControlDelivery { .. } => Err(BrokerError::Internal(
                "unexpected control delivery response".into(),
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
}
