use crate::{
    http::types::{
        AckRequest, PollRequest, PollResponse, PublishControlRequest, PublishEventRequest,
        PublishIngressRequest, PublishRequest, PublishResultRequest, PublishWakeRequest,
        ReceiveControlResponse, ReceiveEventResponse, ReceiveIngressResponse, ReceiveRequest,
        ReceiveResponse, ReceiveResultResponse, ReceiveWakeResponse,
    },
    Broker, BrokerError,
};
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Serialize;
use std::{net::SocketAddr, sync::Arc};
use tokio::net::TcpListener;

struct AppState<B> {
    broker: Arc<B>,
}

impl<B> Clone for AppState<B> {
    fn clone(&self) -> Self {
        Self {
            broker: Arc::clone(&self.broker),
        }
    }
}

pub async fn run_server<B>(addr: SocketAddr, broker: B) -> Result<(), std::io::Error>
where
    B: Broker,
{
    let listener = TcpListener::bind(addr).await?;
    serve(listener, broker).await
}

pub async fn serve<B>(listener: TcpListener, broker: B) -> Result<(), std::io::Error>
where
    B: Broker,
{
    let state = AppState {
        broker: Arc::new(broker),
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/publish", post(publish::<B>))
        .route("/control/publish", post(publish_control::<B>))
        .route("/control/receive", post(receive_control::<B>))
        .route("/control/ack", post(ack_control::<B>))
        .route("/results/publish", post(publish_result::<B>))
        .route("/results/receive", post(receive_result::<B>))
        .route("/results/ack", post(ack_result::<B>))
        .route("/results/nack", post(nack_result::<B>))
        .route("/wake/publish", post(publish_wake::<B>))
        .route("/wake/receive", post(receive_wake::<B>))
        .route("/wake/ack", post(ack_wake::<B>))
        .route("/wake/nack", post(nack_wake::<B>))
        .route("/ingress/publish", post(publish_ingress::<B>))
        .route("/ingress/receive", post(receive_ingress::<B>))
        .route("/ingress/ack", post(ack_ingress::<B>))
        .route("/ingress/nack", post(nack_ingress::<B>))
        .route("/events/publish", post(publish_event::<B>))
        .route("/events/receive", post(receive_event::<B>))
        .route("/receive", post(receive::<B>))
        .route("/poll", post(poll::<B>))
        .route("/ack", post(ack::<B>))
        .route("/nack", post(nack::<B>))
        .with_state(state);

    axum::serve(listener, app).await
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

async fn publish<B>(
    State(state): State<AppState<B>>,
    Json(request): Json<PublishRequest>,
) -> Response
where
    B: Broker,
{
    respond(
        state.broker.publish(request.message).await,
        StatusCode::CREATED,
    )
}

async fn publish_control<B>(
    State(state): State<AppState<B>>,
    Json(request): Json<PublishControlRequest>,
) -> Response
where
    B: Broker,
{
    respond(
        state.broker.publish_control(request.command).await,
        StatusCode::CREATED,
    )
}

async fn receive_control<B>(
    State(state): State<AppState<B>>,
    Json(request): Json<ReceiveRequest>,
) -> Response
where
    B: Broker,
{
    match state.broker.receive_control(&request.consumer).await {
        Ok(delivery) => json_response(StatusCode::OK, ReceiveControlResponse { delivery }),
        Err(err) => error_response(err),
    }
}

async fn publish_result<B>(
    State(state): State<AppState<B>>,
    Json(request): Json<PublishResultRequest>,
) -> Response
where
    B: Broker,
{
    respond(
        state.broker.publish_result(request.message).await,
        StatusCode::CREATED,
    )
}

async fn receive_result<B>(
    State(state): State<AppState<B>>,
    Json(request): Json<ReceiveRequest>,
) -> Response
where
    B: Broker,
{
    match state.broker.receive_result(&request.consumer).await {
        Ok(delivery) => json_response(StatusCode::OK, ReceiveResultResponse { delivery }),
        Err(err) => error_response(err),
    }
}

async fn ack_result<B>(
    State(state): State<AppState<B>>,
    Json(request): Json<AckRequest>,
) -> Response
where
    B: Broker,
{
    respond(
        state
            .broker
            .ack_result(&request.consumer, request.delivery_id)
            .await,
        StatusCode::OK,
    )
}

async fn nack_result<B>(
    State(state): State<AppState<B>>,
    Json(request): Json<AckRequest>,
) -> Response
where
    B: Broker,
{
    respond(
        state
            .broker
            .nack_result(&request.consumer, request.delivery_id)
            .await,
        StatusCode::OK,
    )
}

async fn ack_control<B>(
    State(state): State<AppState<B>>,
    Json(request): Json<AckRequest>,
) -> Response
where
    B: Broker,
{
    respond(
        state
            .broker
            .ack_control(&request.consumer, request.delivery_id)
            .await,
        StatusCode::OK,
    )
}

async fn publish_wake<B>(
    State(state): State<AppState<B>>,
    Json(request): Json<PublishWakeRequest>,
) -> Response
where
    B: Broker,
{
    respond(
        state.broker.publish_wake(request.message).await,
        StatusCode::CREATED,
    )
}

async fn receive_wake<B>(
    State(state): State<AppState<B>>,
    Json(request): Json<ReceiveRequest>,
) -> Response
where
    B: Broker,
{
    match state.broker.receive_wake(&request.consumer).await {
        Ok(delivery) => json_response(StatusCode::OK, ReceiveWakeResponse { delivery }),
        Err(err) => error_response(err),
    }
}

async fn ack_wake<B>(State(state): State<AppState<B>>, Json(request): Json<AckRequest>) -> Response
where
    B: Broker,
{
    respond(
        state
            .broker
            .ack_wake(&request.consumer, request.delivery_id)
            .await,
        StatusCode::OK,
    )
}

async fn nack_wake<B>(State(state): State<AppState<B>>, Json(request): Json<AckRequest>) -> Response
where
    B: Broker,
{
    respond(
        state
            .broker
            .nack_wake(&request.consumer, request.delivery_id)
            .await,
        StatusCode::OK,
    )
}

async fn publish_ingress<B>(
    State(state): State<AppState<B>>,
    Json(request): Json<PublishIngressRequest>,
) -> Response
where
    B: Broker,
{
    respond(
        state.broker.publish_ingress(request.message).await,
        StatusCode::CREATED,
    )
}

async fn receive_ingress<B>(
    State(state): State<AppState<B>>,
    Json(request): Json<ReceiveRequest>,
) -> Response
where
    B: Broker,
{
    match state.broker.receive_ingress(&request.consumer).await {
        Ok(delivery) => json_response(StatusCode::OK, ReceiveIngressResponse { delivery }),
        Err(err) => error_response(err),
    }
}

async fn ack_ingress<B>(
    State(state): State<AppState<B>>,
    Json(request): Json<AckRequest>,
) -> Response
where
    B: Broker,
{
    respond(
        state
            .broker
            .ack_ingress(&request.consumer, request.delivery_id)
            .await,
        StatusCode::OK,
    )
}

async fn nack_ingress<B>(
    State(state): State<AppState<B>>,
    Json(request): Json<AckRequest>,
) -> Response
where
    B: Broker,
{
    respond(
        state
            .broker
            .nack_ingress(&request.consumer, request.delivery_id)
            .await,
        StatusCode::OK,
    )
}

async fn publish_event<B>(
    State(state): State<AppState<B>>,
    Json(request): Json<PublishEventRequest>,
) -> Response
where
    B: Broker,
{
    respond(
        state.broker.publish_event(request.message).await,
        StatusCode::CREATED,
    )
}

async fn receive_event<B>(
    State(state): State<AppState<B>>,
    Json(request): Json<ReceiveRequest>,
) -> Response
where
    B: Broker,
{
    match state.broker.receive_event(&request.consumer).await {
        Ok(delivery) => json_response(StatusCode::OK, ReceiveEventResponse { delivery }),
        Err(err) => error_response(err),
    }
}

async fn receive<B>(
    State(state): State<AppState<B>>,
    Json(request): Json<ReceiveRequest>,
) -> Response
where
    B: Broker,
{
    match state.broker.receive(&request.consumer).await {
        Ok(delivery) => json_response(StatusCode::OK, ReceiveResponse { delivery }),
        Err(err) => error_response(err),
    }
}

async fn poll<B>(State(state): State<AppState<B>>, Json(request): Json<PollRequest>) -> Response
where
    B: Broker,
{
    let poll_result = if let Some(timeout_ms) = request.timeout_ms {
        let broker = state.broker.clone();
        let consumer = request.consumer.clone();
        let timeout = tokio::time::Duration::from_millis(timeout_ms);
        match tokio::time::timeout(timeout, broker.receive(&consumer)).await {
            Ok(result) => result.map(Some),
            Err(_) => Ok(None),
        }
    } else {
        state.broker.receive(&request.consumer).await.map(Some)
    };

    match poll_result {
        Ok(Some(delivery)) => json_response(
            StatusCode::OK,
            PollResponse {
                delivery: Some(delivery),
            },
        ),
        Ok(None) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => error_response(err),
    }
}

async fn ack<B>(State(state): State<AppState<B>>, Json(request): Json<AckRequest>) -> Response
where
    B: Broker,
{
    respond(
        state
            .broker
            .ack(&request.consumer, request.delivery_id)
            .await,
        StatusCode::OK,
    )
}

async fn nack<B>(State(state): State<AppState<B>>, Json(request): Json<AckRequest>) -> Response
where
    B: Broker,
{
    respond(
        state
            .broker
            .nack(&request.consumer, request.delivery_id)
            .await,
        StatusCode::OK,
    )
}

fn respond(result: Result<(), BrokerError>, success: StatusCode) -> Response {
    match result {
        Ok(_) => success.into_response(),
        Err(err) => error_response(err),
    }
}

fn error_response(err: BrokerError) -> Response {
    match err {
        BrokerError::Duplicate(dedupe) => {
            json_response(StatusCode::CONFLICT, ErrorResponse::duplicate(dedupe))
        }
        BrokerError::UnknownDelivery(id) => {
            json_response(StatusCode::NOT_FOUND, ErrorResponse::unknown_delivery(id))
        }
        BrokerError::NotImplemented(context) => json_response(
            StatusCode::NOT_IMPLEMENTED,
            ErrorResponse::new("not_implemented", context),
        ),
        BrokerError::WorkflowResultsUnsupported(message) => json_response(
            StatusCode::NOT_IMPLEMENTED,
            ErrorResponse::new("workflow_results_unsupported", message),
        ),
        BrokerError::Internal(message) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            ErrorResponse::new("internal", message),
        ),
    }
}

fn json_response<T>(status: StatusCode, payload: T) -> Response
where
    T: Serialize,
{
    (status, axum::Json(payload)).into_response()
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

#[derive(Serialize)]
struct ErrorResponse {
    code: &'static str,
    message: String,
}

impl ErrorResponse {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    fn duplicate(key: String) -> Self {
        Self::new("duplicate", key)
    }
    fn unknown_delivery(id: uuid::Uuid) -> Self {
        Self::new("unknown_delivery", id.to_string())
    }
}
