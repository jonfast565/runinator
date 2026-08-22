use crate::{
    http::types::{
        AckRequest, PublishAgentRequest, PublishControlRequest, PublishEffectRequest,
        PublishEffectResultRequest, PublishEventRequest, PublishIngressRequest, PublishWakeRequest,
        ReceiveAgentResponse, ReceiveControlResponse, ReceiveEffectResponse,
        ReceiveEffectResultResponse, ReceiveEventResponse, ReceiveIngressResponse, ReceiveRequest,
        ReceiveWakeResponse,
    },
    AgentCommand, AgentDelivery, Broker, BrokerError, ConsumerProfile, ControlCommand,
    ControlDelivery, EffectDelivery, EffectMessage, EffectResultDelivery, EffectResultMessage,
    EventDelivery, EventMessage, IngressDelivery, IngressMessage, WakeDelivery, WakeMessage,
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

    async fn receive_agent_request(
        &self,
        request: ReceiveRequest,
    ) -> Result<AgentDelivery, BrokerError> {
        let response = self
            .client
            .post(self.endpoint("agent/receive")?)
            .json(&request)
            .send()
            .await
            .map_err(|err| BrokerError::Internal(err.to_string()))?;
        match response.status() {
            StatusCode::OK => response
                .json::<ReceiveAgentResponse>()
                .await
                .map(|payload| payload.delivery)
                .map_err(|err| BrokerError::Internal(err.to_string())),
            status => Err(BrokerError::Internal(format!(
                "unexpected agent receive status: {status}"
            ))),
        }
    }

    async fn receive_effect_request(
        &self,
        path: &str,
        consumer: &str,
        profile: Option<ConsumerProfile>,
    ) -> Result<EffectDelivery, BrokerError> {
        let response = self
            .client
            .post(self.endpoint(path)?)
            .json(&ReceiveRequest {
                consumer: consumer.to_string(),
                profile,
            })
            .send()
            .await
            .map_err(|error| BrokerError::Internal(error.to_string()))?;
        match response.status() {
            StatusCode::OK => response
                .json::<ReceiveEffectResponse>()
                .await
                .map(|payload| payload.delivery)
                .map_err(|error| BrokerError::Internal(error.to_string())),
            status => Err(BrokerError::Internal(format!(
                "unexpected effect receive status: {status}"
            ))),
        }
    }
}

#[async_trait]
impl Broker for HttpBroker {
    fn supports_workflow_effect_channels(&self) -> bool {
        true
    }

    fn supports_agent_channel(&self) -> bool {
        true
    }

    async fn heartbeat(&self) -> Result<(), BrokerError> {
        let response = self
            .client
            .get(self.endpoint("health")?)
            .send()
            .await
            .map_err(|err| BrokerError::Internal(err.to_string()))?;
        match response.status() {
            StatusCode::OK => Ok(()),
            status => Err(BrokerError::Internal(format!(
                "unexpected broker health status: {status}"
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
                profile: None,
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

    async fn receive_control_for(
        &self,
        profile: &ConsumerProfile,
    ) -> Result<ControlDelivery, BrokerError> {
        let url = self.endpoint("control/receive")?;
        let response = self
            .client
            .post(url)
            .json(&ReceiveRequest {
                consumer: profile.id.clone(),
                profile: Some(profile.clone()),
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

    async fn nack_control(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        self.post_ack("control/nack", consumer, delivery_id).await
    }

    async fn publish_agent(&self, command: AgentCommand) -> Result<(), BrokerError> {
        let response = self
            .client
            .post(self.endpoint("agent/publish")?)
            .json(&PublishAgentRequest { command })
            .send()
            .await
            .map_err(|err| BrokerError::Internal(err.to_string()))?;
        match response.status() {
            StatusCode::OK | StatusCode::CREATED => Ok(()),
            status => Err(BrokerError::Internal(format!(
                "unexpected agent publish status: {status}"
            ))),
        }
    }

    async fn receive_agent(&self, consumer: &str) -> Result<AgentDelivery, BrokerError> {
        self.receive_agent_request(ReceiveRequest {
            consumer: consumer.to_string(),
            profile: None,
        })
        .await
    }

    async fn receive_agent_for(
        &self,
        profile: &ConsumerProfile,
    ) -> Result<AgentDelivery, BrokerError> {
        self.receive_agent_request(ReceiveRequest {
            consumer: profile.id.clone(),
            profile: Some(profile.clone()),
        })
        .await
    }

    async fn ack_agent(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        self.post_ack("agent/ack", consumer, delivery_id).await
    }

    async fn nack_agent(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        self.post_ack("agent/nack", consumer, delivery_id).await
    }

    async fn publish_effect(&self, message: EffectMessage) -> Result<(), BrokerError> {
        let response = self
            .client
            .post(self.endpoint("effects/publish")?)
            .json(&PublishEffectRequest { message })
            .send()
            .await
            .map_err(|error| BrokerError::Internal(error.to_string()))?;
        match response.status() {
            StatusCode::OK | StatusCode::CREATED => Ok(()),
            status => Err(BrokerError::Internal(format!(
                "unexpected effect publish status: {status}"
            ))),
        }
    }

    async fn receive_effect(&self, consumer: &str) -> Result<EffectDelivery, BrokerError> {
        self.receive_effect_request("effects/receive", consumer, None)
            .await
    }

    async fn receive_effect_for(
        &self,
        profile: &ConsumerProfile,
    ) -> Result<EffectDelivery, BrokerError> {
        self.receive_effect_request("effects/receive", &profile.id, Some(profile.clone()))
            .await
    }

    async fn receive_infrastructure_effect(
        &self,
        consumer: &str,
    ) -> Result<EffectDelivery, BrokerError> {
        self.receive_effect_request("effects/infrastructure/receive", consumer, None)
            .await
    }

    async fn ack_effect(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        self.post_ack("effects/ack", consumer, delivery_id).await
    }

    async fn nack_effect(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        self.post_ack("effects/nack", consumer, delivery_id).await
    }

    async fn publish_effect_result(&self, message: EffectResultMessage) -> Result<(), BrokerError> {
        let response = self
            .client
            .post(self.endpoint("effect-results/publish")?)
            .json(&PublishEffectResultRequest { message })
            .send()
            .await
            .map_err(|error| BrokerError::Internal(error.to_string()))?;
        match response.status() {
            StatusCode::OK | StatusCode::CREATED => Ok(()),
            status => Err(BrokerError::Internal(format!(
                "unexpected effect-result publish status: {status}"
            ))),
        }
    }

    async fn receive_effect_result(
        &self,
        consumer: &str,
    ) -> Result<EffectResultDelivery, BrokerError> {
        let response = self
            .client
            .post(self.endpoint("effect-results/receive")?)
            .json(&ReceiveRequest {
                consumer: consumer.to_string(),
                profile: None,
            })
            .send()
            .await
            .map_err(|error| BrokerError::Internal(error.to_string()))?;
        match response.status() {
            StatusCode::OK => response
                .json::<ReceiveEffectResultResponse>()
                .await
                .map(|payload| payload.delivery)
                .map_err(|error| BrokerError::Internal(error.to_string())),
            status => Err(BrokerError::Internal(format!(
                "unexpected effect-result receive status: {status}"
            ))),
        }
    }

    async fn ack_effect_result(
        &self,
        consumer: &str,
        delivery_id: Uuid,
    ) -> Result<(), BrokerError> {
        self.post_ack("effect-results/ack", consumer, delivery_id)
            .await
    }

    async fn nack_effect_result(
        &self,
        consumer: &str,
        delivery_id: Uuid,
    ) -> Result<(), BrokerError> {
        self.post_ack("effect-results/nack", consumer, delivery_id)
            .await
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
                profile: None,
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
                profile: None,
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

    async fn publish_event(&self, message: EventMessage) -> Result<(), BrokerError> {
        let url = self.endpoint("events/publish")?;
        let response = self
            .client
            .post(url)
            .json(&PublishEventRequest { message })
            .send()
            .await
            .map_err(|err| BrokerError::Internal(err.to_string()))?;

        match response.status() {
            StatusCode::OK | StatusCode::CREATED => Ok(()),
            status => Err(BrokerError::Internal(format!(
                "unexpected event publish status: {status}"
            ))),
        }
    }

    async fn receive_event(&self, consumer: &str) -> Result<EventDelivery, BrokerError> {
        let url = self.endpoint("events/receive")?;
        let response = self
            .client
            .post(url)
            .json(&ReceiveRequest {
                consumer: consumer.to_string(),
                profile: None,
            })
            .send()
            .await
            .map_err(|err| BrokerError::Internal(err.to_string()))?;

        match response.status() {
            StatusCode::OK => {
                let payload = response
                    .json::<ReceiveEventResponse>()
                    .await
                    .map_err(|err| BrokerError::Internal(err.to_string()))?;
                Ok(payload.delivery)
            }
            status => Err(BrokerError::Internal(format!(
                "unexpected event receive status: {status}"
            ))),
        }
    }
}
