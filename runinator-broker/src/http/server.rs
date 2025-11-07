use crate::{
    http::types::{AckRequest, PollRequest, PollResponse, PublishRequest},
    Broker, BrokerError,
};
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
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
    let state = AppState {
        broker: Arc::new(broker),
    };

    let app = Router::new()
        .route("/publish", post(publish::<B>))
        .route("/poll", post(poll::<B>))
        .route("/ack", post(ack::<B>))
        .route("/nack", post(nack::<B>))
        .with_state(state);

    axum::serve(listener, app).await
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

async fn poll<B>(State(state): State<AppState<B>>, Json(request): Json<PollRequest>) -> Response
where
    B: Broker,
{
    let poll_result = if let Some(timeout_ms) = request.timeout_ms {
        let broker = state.broker.clone();
        let consumer = request.consumer.clone();
        let timeout = tokio::time::Duration::from_millis(timeout_ms);
        match tokio::time::timeout(timeout, broker.poll(&consumer)).await {
            Ok(result) => result,
            Err(_) => Ok(None),
        }
    } else {
        state.broker.poll(&request.consumer).await
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
