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
#[path = "capabilities_tests.rs"]
mod tests;
