//! shared request/response dispatch for any transport built on [`crate::tcp::types::TcpRequest`]/
//! [`crate::tcp::types::TcpResponse`] (the tcp transport, and the ws relay, which wraps the same
//! enums in a request-id envelope for multiplexing over one persistent connection). kept
//! transport-agnostic: no I/O here, just "given a decoded request, call the matching `Broker` method
//! and encode the result."

use crate::{
    tcp::types::{TcpRequest, TcpResponse},
    Broker,
};

/// run one request against `broker`, returning the response to send back. never fails: any error
/// from the broker call itself is encoded as [`TcpResponse::Error`] rather than propagated, since
/// every transport using this always owes the peer exactly one response per request.
///
/// takes `&dyn Broker` (not a generic `B: Broker`) so it accepts `runinator-ws`'s `Arc<dyn Broker>`
/// directly — a generic bound would implicitly require `Sized`, which a trait object never is. a
/// concrete `&ConcreteBroker` (as `tcp`/`ws`'s standalone servers hold) still coerces to `&dyn Broker`
/// at the call site same as any other unsizing coercion.
pub async fn dispatch(broker: &dyn Broker, request: TcpRequest) -> TcpResponse {
    let result = match request {
        TcpRequest::Publish { message } => broker.publish(message).await.map(|_| TcpResponse::Ok),
        TcpRequest::PublishControl { command } => broker
            .publish_control(command)
            .await
            .map(|_| TcpResponse::Ok),
        TcpRequest::PublishResult { message } => broker
            .publish_result(message)
            .await
            .map(|_| TcpResponse::Ok),
        TcpRequest::Receive { consumer } => broker
            .receive(&consumer)
            .await
            .map(|delivery| TcpResponse::Delivery { delivery }),
        TcpRequest::ReceiveFor { profile } => broker
            .receive_for(&profile)
            .await
            .map(|delivery| TcpResponse::Delivery { delivery }),
        TcpRequest::ReceiveControl { consumer } => broker
            .receive_control(&consumer)
            .await
            .map(|delivery| TcpResponse::ControlDelivery { delivery }),
        TcpRequest::ReceiveResult { consumer } => broker
            .receive_result(&consumer)
            .await
            .map(|delivery| TcpResponse::ResultDelivery { delivery }),
        TcpRequest::Ack {
            consumer,
            delivery_id,
        } => broker
            .ack(&consumer, delivery_id)
            .await
            .map(|_| TcpResponse::Ok),
        TcpRequest::AckControl {
            consumer,
            delivery_id,
        } => broker
            .ack_control(&consumer, delivery_id)
            .await
            .map(|_| TcpResponse::Ok),
        TcpRequest::AckResult {
            consumer,
            delivery_id,
        } => broker
            .ack_result(&consumer, delivery_id)
            .await
            .map(|_| TcpResponse::Ok),
        TcpRequest::Nack {
            consumer,
            delivery_id,
        } => broker
            .nack(&consumer, delivery_id)
            .await
            .map(|_| TcpResponse::Ok),
        TcpRequest::NackResult {
            consumer,
            delivery_id,
        } => broker
            .nack_result(&consumer, delivery_id)
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
