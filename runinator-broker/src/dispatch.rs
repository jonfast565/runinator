//! Shared request/response dispatch for TCP and the WS relay.
//! It performs no I/O. It receives a decoded request, calls the matching `Broker` method, and
//! encodes the result.

use crate::{
    tcp::types::{TcpRequest, TcpResponse},
    Broker,
};

/// Run one request against `broker` and return the response.
/// Broker errors become [`TcpResponse::Error`] so every request gets one response.
///
/// Take `&dyn Broker` so the web service can pass its `Arc<dyn Broker>` directly.
/// Concrete brokers coerce to the same trait-object reference at the call site.
pub async fn dispatch(broker: &dyn Broker, request: TcpRequest) -> TcpResponse {
    let result = match request {
        TcpRequest::PublishControl { command } => broker
            .publish_control(command)
            .await
            .map(|_| TcpResponse::Ok),
        TcpRequest::PublishAgent { command } => {
            broker.publish_agent(command).await.map(|_| TcpResponse::Ok)
        }
        TcpRequest::PublishEffect { message } => broker
            .publish_effect(message)
            .await
            .map(|_| TcpResponse::Ok),
        TcpRequest::PublishEffectResult { message } => broker
            .publish_effect_result(message)
            .await
            .map(|_| TcpResponse::Ok),
        TcpRequest::ReceiveControl { consumer } => broker
            .receive_control(&consumer)
            .await
            .map(|delivery| TcpResponse::ControlDelivery { delivery }),
        TcpRequest::ReceiveControlFor { profile } => broker
            .receive_control_for(&profile)
            .await
            .map(|delivery| TcpResponse::ControlDelivery { delivery }),
        TcpRequest::ReceiveAgent { consumer } => broker
            .receive_agent(&consumer)
            .await
            .map(|delivery| TcpResponse::AgentDelivery { delivery }),
        TcpRequest::ReceiveAgentFor { profile } => broker
            .receive_agent_for(&profile)
            .await
            .map(|delivery| TcpResponse::AgentDelivery { delivery }),
        TcpRequest::ReceiveEffect { consumer } => broker
            .receive_effect(&consumer)
            .await
            .map(|delivery| TcpResponse::EffectDelivery { delivery }),
        TcpRequest::ReceiveEffectFor { profile } => broker
            .receive_effect_for(&profile)
            .await
            .map(|delivery| TcpResponse::EffectDelivery { delivery }),
        TcpRequest::ReceiveInfrastructureEffect { consumer } => broker
            .receive_infrastructure_effect(&consumer)
            .await
            .map(|delivery| TcpResponse::EffectDelivery { delivery }),
        TcpRequest::ReceiveEffectResult { consumer } => broker
            .receive_effect_result(&consumer)
            .await
            .map(|delivery| TcpResponse::EffectResultDelivery { delivery }),
        TcpRequest::AckControl {
            consumer,
            delivery_id,
        } => broker
            .ack_control(&consumer, delivery_id)
            .await
            .map(|_| TcpResponse::Ok),
        TcpRequest::AckAgent {
            consumer,
            delivery_id,
        } => broker
            .ack_agent(&consumer, delivery_id)
            .await
            .map(|_| TcpResponse::Ok),
        TcpRequest::AckEffect {
            consumer,
            delivery_id,
        } => broker
            .ack_effect(&consumer, delivery_id)
            .await
            .map(|_| TcpResponse::Ok),
        TcpRequest::AckEffectResult {
            consumer,
            delivery_id,
        } => broker
            .ack_effect_result(&consumer, delivery_id)
            .await
            .map(|_| TcpResponse::Ok),
        TcpRequest::NackControl {
            consumer,
            delivery_id,
        } => broker
            .nack_control(&consumer, delivery_id)
            .await
            .map(|_| TcpResponse::Ok),
        TcpRequest::NackAgent {
            consumer,
            delivery_id,
        } => broker
            .nack_agent(&consumer, delivery_id)
            .await
            .map(|_| TcpResponse::Ok),
        TcpRequest::NackEffect {
            consumer,
            delivery_id,
        } => broker
            .nack_effect(&consumer, delivery_id)
            .await
            .map(|_| TcpResponse::Ok),
        TcpRequest::NackEffectResult {
            consumer,
            delivery_id,
        } => broker
            .nack_effect_result(&consumer, delivery_id)
            .await
            .map(|_| TcpResponse::Ok),
        TcpRequest::PublishWake { message } => {
            broker.publish_wake(message).await.map(|_| TcpResponse::Ok)
        }
        TcpRequest::PublishIngress { message } => broker
            .publish_ingress(message)
            .await
            .map(|_| TcpResponse::Ok),
        TcpRequest::ReceiveWake { consumer } => broker
            .receive_wake(&consumer)
            .await
            .map(|delivery| TcpResponse::WakeDelivery { delivery }),
        TcpRequest::ReceiveIngress { consumer } => broker
            .receive_ingress(&consumer)
            .await
            .map(|delivery| TcpResponse::IngressDelivery { delivery }),
        TcpRequest::AckWake {
            consumer,
            delivery_id,
        } => broker
            .ack_wake(&consumer, delivery_id)
            .await
            .map(|_| TcpResponse::Ok),
        TcpRequest::AckIngress {
            consumer,
            delivery_id,
        } => broker
            .ack_ingress(&consumer, delivery_id)
            .await
            .map(|_| TcpResponse::Ok),
        TcpRequest::NackWake {
            consumer,
            delivery_id,
        } => broker
            .nack_wake(&consumer, delivery_id)
            .await
            .map(|_| TcpResponse::Ok),
        TcpRequest::NackIngress {
            consumer,
            delivery_id,
        } => broker
            .nack_ingress(&consumer, delivery_id)
            .await
            .map(|_| TcpResponse::Ok),
        TcpRequest::PublishEvent { message } => {
            broker.publish_event(message).await.map(|_| TcpResponse::Ok)
        }
        TcpRequest::ReceiveEvent { consumer } => broker
            .receive_event(&consumer)
            .await
            .map(|delivery| TcpResponse::EventDelivery { delivery }),
    };
    result.unwrap_or_else(|err| TcpResponse::Error {
        message: err.to_string(),
    })
}
