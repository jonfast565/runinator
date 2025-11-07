use crate::{Broker, BrokerDelivery, BrokerError, BrokerMessage};
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

    async fn poll(&self, _consumer: &str) -> Result<Option<BrokerDelivery>, BrokerError> {
        Err(BrokerError::NotImplemented("kafka poll"))
    }

    async fn ack(&self, _consumer: &str, _delivery_id: uuid::Uuid) -> Result<(), BrokerError> {
        Err(BrokerError::NotImplemented("kafka ack"))
    }

    async fn nack(&self, _consumer: &str, _delivery_id: uuid::Uuid) -> Result<(), BrokerError> {
        Err(BrokerError::NotImplemented("kafka nack"))
    }
}
