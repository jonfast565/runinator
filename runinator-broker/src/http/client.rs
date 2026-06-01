use crate::{
    http::types::{
        AckRequest, PublishControlRequest, PublishIngressRequest, PublishRequest,
        PublishWakeRequest, ReceiveControlResponse, ReceiveIngressResponse, ReceiveRequest,
        ReceiveResponse, ReceiveResultResponse, ReceiveWakeResponse,
    },
    Broker, BrokerDelivery, BrokerError, BrokerMessage, ControlCommand, ControlDelivery,
    IngressDelivery, IngressMessage, ResultDelivery, ResultMessage, WakeDelivery, WakeMessage,
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

    async fn post_ack(
        &self,
        path: &str,
        consumer: &str,
        delivery_id: Uuid,
    ) -> Result<(), BrokerError> {
        let url = self.endpoint(path)?;
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
                "unexpected {path} status: {status}"
            ))),
        }
    }
}

#[async_trait]
impl Broker for HttpBroker {
    fn supports_workflow_result_channels(&self) -> bool {
        true
    }

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

    async fn publish_control(&self, command: ControlCommand) -> Result<(), BrokerError> {
        let url = self.endpoint("control/publish")?;
        let response = self
            .client
            .post(url)
            .json(&PublishControlRequest { command })
            .send()
            .await
            .map_err(|err| BrokerError::Internal(err.to_string()))?;

        match response.status() {
            StatusCode::OK | StatusCode::CREATED => Ok(()),
            status => Err(BrokerError::Internal(format!(
                "unexpected control publish status: {status}"
            ))),
        }
    }

    async fn receive_control(&self, consumer: &str) -> Result<ControlDelivery, BrokerError> {
        let url = self.endpoint("control/receive")?;
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
                    .json::<ReceiveControlResponse>()
                    .await
                    .map_err(|err| BrokerError::Internal(err.to_string()))?;
                Ok(payload.delivery)
            }
            status => Err(BrokerError::Internal(format!(
                "unexpected control receive status: {status}"
            ))),
        }
    }

    async fn ack_control(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        self.post_ack("control/ack", consumer, delivery_id).await
    }

    async fn publish_result(&self, message: ResultMessage) -> Result<(), BrokerError> {
        let url = self.endpoint("results/publish")?;
        let dedupe_key = message.dedupe_key_or_hash();
        let response = self
            .client
            .post(url)
            .json(&crate::http::types::PublishResultRequest { message })
            .send()
            .await
            .map_err(|err| BrokerError::Internal(err.to_string()))?;

        match response.status() {
            StatusCode::OK | StatusCode::CREATED => Ok(()),
            StatusCode::CONFLICT => Err(BrokerError::Duplicate(dedupe_key)),
            status => Err(BrokerError::Internal(format!(
                "unexpected result publish status: {status}"
            ))),
        }
    }

    async fn receive_result(&self, consumer: &str) -> Result<ResultDelivery, BrokerError> {
        let url = self.endpoint("results/receive")?;
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
                    .json::<ReceiveResultResponse>()
                    .await
                    .map_err(|err| BrokerError::Internal(err.to_string()))?;
                Ok(payload.delivery)
            }
            status => Err(BrokerError::Internal(format!(
                "unexpected result receive status: {status}"
            ))),
        }
    }

    async fn ack_result(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        self.post_ack("results/ack", consumer, delivery_id).await
    }

    async fn nack_result(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        self.post_ack("results/nack", consumer, delivery_id).await
    }

    async fn publish_wake(&self, message: WakeMessage) -> Result<(), BrokerError> {
        let url = self.endpoint("wake/publish")?;
        let dedupe_key = message.dedupe_key_or_hash();
        let response = self
            .client
            .post(url)
            .json(&PublishWakeRequest { message })
            .send()
            .await
            .map_err(|err| BrokerError::Internal(err.to_string()))?;

        match response.status() {
            StatusCode::OK | StatusCode::CREATED => Ok(()),
            StatusCode::CONFLICT => Err(BrokerError::Duplicate(dedupe_key)),
            status => Err(BrokerError::Internal(format!(
                "unexpected wake publish status: {status}"
            ))),
        }
    }

    async fn receive_wake(&self, consumer: &str) -> Result<WakeDelivery, BrokerError> {
        let url = self.endpoint("wake/receive")?;
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
                    .json::<ReceiveWakeResponse>()
                    .await
                    .map_err(|err| BrokerError::Internal(err.to_string()))?;
                Ok(payload.delivery)
            }
            status => Err(BrokerError::Internal(format!(
                "unexpected wake receive status: {status}"
            ))),
        }
    }

    async fn ack_wake(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        self.post_ack("wake/ack", consumer, delivery_id).await
    }

    async fn nack_wake(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        self.post_ack("wake/nack", consumer, delivery_id).await
    }

    async fn publish_ingress(&self, message: IngressMessage) -> Result<(), BrokerError> {
        let url = self.endpoint("ingress/publish")?;
        let dedupe_key = message.dedupe_key_or_hash();
        let response = self
            .client
            .post(url)
            .json(&PublishIngressRequest { message })
            .send()
            .await
            .map_err(|err| BrokerError::Internal(err.to_string()))?;

        match response.status() {
            StatusCode::OK | StatusCode::CREATED => Ok(()),
            StatusCode::CONFLICT => Err(BrokerError::Duplicate(dedupe_key)),
            status => Err(BrokerError::Internal(format!(
                "unexpected ingress publish status: {status}"
            ))),
        }
    }

    async fn receive_ingress(&self, consumer: &str) -> Result<IngressDelivery, BrokerError> {
        let url = self.endpoint("ingress/receive")?;
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
                    .json::<ReceiveIngressResponse>()
                    .await
                    .map_err(|err| BrokerError::Internal(err.to_string()))?;
                Ok(payload.delivery)
            }
            status => Err(BrokerError::Internal(format!(
                "unexpected ingress receive status: {status}"
            ))),
        }
    }

    async fn ack_ingress(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        self.post_ack("ingress/ack", consumer, delivery_id).await
    }

    async fn nack_ingress(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        self.post_ack("ingress/nack", consumer, delivery_id).await
    }
}
