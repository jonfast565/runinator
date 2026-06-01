use crate::{Broker, BrokerError};

pub fn ensure_named_workflow_result_channel(
    backend: &str,
    result_channel: &str,
) -> Result<(), BrokerError> {
    let channel_kind = match backend {
        "kafka" => "topic",
        "rabbitmq" => "queue",
        _ => return Ok(()),
    };

    if !result_channel.trim().is_empty() {
        return Ok(());
    }

    Err(BrokerError::WorkflowResultsUnsupported(format!(
        "Broker backend '{backend}' requires a non-empty workflow result {channel_kind} (--broker-result-topic) before brokered worker results can be used"
    )))
}

pub fn ensure_workflow_result_channels_supported(
    backend: &str,
    broker: &dyn Broker,
) -> Result<(), BrokerError> {
    if broker.supports_workflow_result_channels() {
        return Ok(());
    }

    Err(BrokerError::WorkflowResultsUnsupported(format!(
        "Broker backend '{backend}' does not support workflow result channels; brokered worker results require result publish, receive, ack, and nack support"
    )))
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use uuid::Uuid;

    use super::*;
    use crate::{
        in_memory::InMemoryBroker, BrokerDelivery, BrokerMessage, ControlCommand, ControlDelivery,
        IngressDelivery, IngressMessage, ResultDelivery, ResultMessage, WakeDelivery, WakeMessage,
    };

    #[test]
    fn kafka_requires_named_workflow_result_topic() {
        let err = ensure_named_workflow_result_channel("kafka", " ").unwrap_err();

        assert!(err.to_string().contains("Broker backend 'kafka'"));
        assert!(err.to_string().contains("non-empty workflow result topic"));
    }

    #[test]
    fn rabbitmq_requires_named_workflow_result_queue() {
        let err = ensure_named_workflow_result_channel("rabbitmq", "").unwrap_err();

        assert!(err.to_string().contains("Broker backend 'rabbitmq'"));
        assert!(err.to_string().contains("non-empty workflow result queue"));
    }

    #[test]
    fn non_direct_backends_do_not_require_named_result_channels() {
        ensure_named_workflow_result_channel("tcp", "").unwrap();
        ensure_named_workflow_result_channel("http", "").unwrap();
        ensure_named_workflow_result_channel("in-memory", "").unwrap();
    }

    #[test]
    fn unsupported_broker_returns_clear_result_channel_error() {
        let err =
            ensure_workflow_result_channels_supported("custom", &UnsupportedBroker).unwrap_err();

        assert!(err.to_string().contains("Broker backend 'custom'"));
        assert!(err
            .to_string()
            .contains("does not support workflow result channels"));
    }

    #[test]
    fn supported_broker_passes_result_channel_guard() {
        ensure_workflow_result_channels_supported("in-memory", &InMemoryBroker::new()).unwrap();
    }

    struct UnsupportedBroker;

    #[async_trait]
    impl Broker for UnsupportedBroker {
        async fn publish(&self, _message: BrokerMessage) -> Result<(), BrokerError> {
            unreachable!()
        }

        async fn receive(&self, _consumer: &str) -> Result<BrokerDelivery, BrokerError> {
            unreachable!()
        }

        async fn ack(&self, _consumer: &str, _delivery_id: Uuid) -> Result<(), BrokerError> {
            unreachable!()
        }

        async fn nack(&self, _consumer: &str, _delivery_id: Uuid) -> Result<(), BrokerError> {
            unreachable!()
        }

        async fn publish_control(&self, _command: ControlCommand) -> Result<(), BrokerError> {
            unreachable!()
        }

        async fn receive_control(&self, _consumer: &str) -> Result<ControlDelivery, BrokerError> {
            unreachable!()
        }

        async fn ack_control(
            &self,
            _consumer: &str,
            _delivery_id: Uuid,
        ) -> Result<(), BrokerError> {
            unreachable!()
        }

        async fn publish_result(&self, _message: ResultMessage) -> Result<(), BrokerError> {
            unreachable!()
        }

        async fn receive_result(&self, _consumer: &str) -> Result<ResultDelivery, BrokerError> {
            unreachable!()
        }

        async fn ack_result(&self, _consumer: &str, _delivery_id: Uuid) -> Result<(), BrokerError> {
            unreachable!()
        }

        async fn nack_result(
            &self,
            _consumer: &str,
            _delivery_id: Uuid,
        ) -> Result<(), BrokerError> {
            unreachable!()
        }

        async fn publish_wake(&self, _message: WakeMessage) -> Result<(), BrokerError> {
            unreachable!()
        }

        async fn receive_wake(&self, _consumer: &str) -> Result<WakeDelivery, BrokerError> {
            unreachable!()
        }

        async fn ack_wake(&self, _consumer: &str, _delivery_id: Uuid) -> Result<(), BrokerError> {
            unreachable!()
        }

        async fn nack_wake(&self, _consumer: &str, _delivery_id: Uuid) -> Result<(), BrokerError> {
            unreachable!()
        }

        async fn publish_ingress(&self, _message: IngressMessage) -> Result<(), BrokerError> {
            unreachable!()
        }

        async fn receive_ingress(&self, _consumer: &str) -> Result<IngressDelivery, BrokerError> {
            unreachable!()
        }

        async fn ack_ingress(
            &self,
            _consumer: &str,
            _delivery_id: Uuid,
        ) -> Result<(), BrokerError> {
            unreachable!()
        }

        async fn nack_ingress(
            &self,
            _consumer: &str,
            _delivery_id: Uuid,
        ) -> Result<(), BrokerError> {
            unreachable!()
        }
    }
}
