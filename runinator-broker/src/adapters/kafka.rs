use crate::{
    Broker, BrokerDelivery, BrokerError, BrokerMessage, ControlCommand, ControlDelivery,
    ResultDelivery, ResultMessage,
};
use async_trait::async_trait;

pub struct KafkaBroker;

impl KafkaBroker {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Broker for KafkaBroker {
    async fn publish(&self, _message: BrokerMessage) -> Result<(), BrokerError> {
        Err(BrokerError::NotImplemented("kafka publish"))
    }

    async fn receive(&self, _consumer: &str) -> Result<BrokerDelivery, BrokerError> {
        Err(BrokerError::NotImplemented("kafka receive"))
    }

    async fn ack(&self, _consumer: &str, _delivery_id: uuid::Uuid) -> Result<(), BrokerError> {
        Err(BrokerError::NotImplemented("kafka ack"))
    }

    async fn nack(&self, _consumer: &str, _delivery_id: uuid::Uuid) -> Result<(), BrokerError> {
        Err(BrokerError::NotImplemented("kafka nack"))
    }

    async fn publish_control(&self, _message: ControlCommand) -> Result<(), BrokerError> {
        Err(BrokerError::NotImplemented("kafka publish_control"))
    }

    async fn receive_control(&self, _consumer: &str) -> Result<ControlDelivery, BrokerError> {
        Err(BrokerError::NotImplemented("kafka receive_control"))
    }

    async fn ack_control(
        &self,
        _consumer: &str,
        _delivery_id: uuid::Uuid,
    ) -> Result<(), BrokerError> {
        Err(BrokerError::NotImplemented("kafka ack_control"))
    }

    async fn publish_result(&self, _message: ResultMessage) -> Result<(), BrokerError> {
        Err(BrokerError::NotImplemented("kafka publish_result"))
    }

    async fn receive_result(&self, _consumer: &str) -> Result<ResultDelivery, BrokerError> {
        Err(BrokerError::NotImplemented("kafka receive_result"))
    }

    async fn ack_result(
        &self,
        _consumer: &str,
        _delivery_id: uuid::Uuid,
    ) -> Result<(), BrokerError> {
        Err(BrokerError::NotImplemented("kafka ack_result"))
    }

    async fn nack_result(
        &self,
        _consumer: &str,
        _delivery_id: uuid::Uuid,
    ) -> Result<(), BrokerError> {
        Err(BrokerError::NotImplemented("kafka nack_result"))
    }
}
