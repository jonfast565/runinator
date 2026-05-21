use crate::{Broker, BrokerDelivery, BrokerError, BrokerMessage};
use async_trait::async_trait;

pub struct RabbitMqBroker;

impl RabbitMqBroker {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Broker for RabbitMqBroker {
    async fn publish(&self, _message: BrokerMessage) -> Result<(), BrokerError> {
        Err(BrokerError::NotImplemented("rabbitmq publish"))
    }

    async fn receive(&self, _consumer: &str) -> Result<BrokerDelivery, BrokerError> {
        Err(BrokerError::NotImplemented("rabbitmq receive"))
    }

    async fn ack(&self, _consumer: &str, _delivery_id: uuid::Uuid) -> Result<(), BrokerError> {
        Err(BrokerError::NotImplemented("rabbitmq ack"))
    }

    async fn nack(&self, _consumer: &str, _delivery_id: uuid::Uuid) -> Result<(), BrokerError> {
        Err(BrokerError::NotImplemented("rabbitmq nack"))
    }
}
