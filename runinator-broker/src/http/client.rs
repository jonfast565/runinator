use crate::{
    http::types::{AckRequest, PublishRequest, ReceiveRequest, ReceiveResponse},
    Broker, BrokerDelivery, BrokerError, BrokerMessage,
};
use async_trait::async_trait;
use reqwest::{Client, StatusCode, Url};
use uuid::Uuid;

#[derive(Clone)]
pub struct HttpBroker {
    client: Client,
    base_url: Url,
}

impl HttpBroker {
    pub fn new(base_url: Url, client: Client) -> Self {
        Self { client, base_url }
    }

    fn endpoint(&self, path: &str) -> Result<Url, BrokerError> {
        self.base_url
            .join(path)
            .map_err(|err| BrokerError::Internal(err.to_string()))
    }
}

#[async_trait]
impl Broker for HttpBroker {
    async fn publish(&self, message: BrokerMessage) -> Result<(), BrokerError> {
        let url = self.endpoint("publish")?;
        let dedupe_key = message.dedupe_key_or_hash();
        let response = self
            .client
            .post(url)
            .json(&PublishRequest { message })
            .send()
            .await
            .map_err(|err| BrokerError::Internal(err.to_string()))?;

        match response.status() {
            StatusCode::OK | StatusCode::CREATED => Ok(()),
            StatusCode::CONFLICT => Err(BrokerError::Duplicate(dedupe_key)),
            status => Err(BrokerError::Internal(format!(
                "unexpected publish status: {status}"
            ))),
        }
    }

    async fn receive(&self, consumer: &str) -> Result<BrokerDelivery, BrokerError> {
        let url = self.endpoint("receive")?;
        let response = self
            .client
            .post(url)
            .json(&ReceiveRequest {
                consumer: consumer.to_string(),
            })
            .send()
            .await
            .map_err(|err| BrokerError::Internal(err.to_string()))?;

        match response.status() {
            StatusCode::OK => {
                let payload = response
                    .json::<ReceiveResponse>()
                    .await
                    .map_err(|err| BrokerError::Internal(err.to_string()))?;
                Ok(payload.delivery)
            }
            status => Err(BrokerError::Internal(format!(
                "unexpected receive status: {status}"
            ))),
        }
    }

    async fn ack(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        let url = self.endpoint("ack")?;
        let response = self
            .client
            .post(url)
            .json(&AckRequest {
                consumer: consumer.to_string(),
                delivery_id,
            })
            .send()
            .await
            .map_err(|err| BrokerError::Internal(err.to_string()))?;
        match response.status() {
            StatusCode::OK => Ok(()),
            status => Err(BrokerError::Internal(format!(
                "unexpected ack status: {status}"
            ))),
        }
    }

    async fn nack(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        let url = self.endpoint("nack")?;
        let response = self
            .client
            .post(url)
            .json(&AckRequest {
                consumer: consumer.to_string(),
                delivery_id,
            })
            .send()
            .await
            .map_err(|err| BrokerError::Internal(err.to_string()))?;
        match response.status() {
            StatusCode::OK => Ok(()),
            status => Err(BrokerError::Internal(format!(
                "unexpected nack status: {status}"
            ))),
        }
    }
}
