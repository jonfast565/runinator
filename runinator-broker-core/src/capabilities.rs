use crate::{Broker, BrokerError};

pub fn ensure_named_workflow_effect_channel(
    backend: &str,
    effect_channel: &str,
) -> Result<(), BrokerError> {
    let channel_kind = match backend {
        "kafka" => "topic",
        "rabbitmq" => "queue",
        _ => return Ok(()),
    };

    if !effect_channel.trim().is_empty() {
        return Ok(());
    }

    Err(BrokerError::WorkflowEffectsUnsupported(format!(
        "Broker backend '{backend}' requires a non-empty workflow effect {channel_kind} (--broker-effect-topic) before workflow effects can be dispatched"
    )))
}

pub fn ensure_workflow_effect_channels_supported(
    backend: &str,
    broker: &dyn Broker,
) -> Result<(), BrokerError> {
    if broker.supports_workflow_effect_channels() {
        return Ok(());
    }

    Err(BrokerError::WorkflowEffectsUnsupported(format!(
        "Broker backend '{backend}' does not support workflow effect channels; the workflow VM requires effect publish, receive, ack, and nack support"
    )))
}

pub fn ensure_agent_channel_supported(
    backend: &str,
    broker: &dyn Broker,
) -> Result<(), BrokerError> {
    if broker.supports_agent_channel() {
        return Ok(());
    }
    Err(BrokerError::NotImplemented(match backend {
        "kafka" => "agent topic",
        "rabbitmq" => "agent queue",
        _ => "agent channel",
    }))
}

#[cfg(test)]
#[path = "capabilities_tests.rs"]
mod tests;
