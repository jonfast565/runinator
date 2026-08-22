use crate::{
    http::auth::{AuthIdentity, BrokerAuth},
    http::types::{
        AckRequest, PublishAgentRequest, PublishControlRequest, PublishEffectRequest,
        PublishEffectResultRequest, PublishEventRequest, PublishIngressRequest, PublishWakeRequest,
        ReceiveAgentResponse, ReceiveControlResponse, ReceiveEffectResponse,
        ReceiveEffectResultResponse, ReceiveEventResponse, ReceiveIngressResponse, ReceiveRequest,
        ReceiveWakeResponse,
    },
    Broker, BrokerError, ConsumerProfile,
};
use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
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

/// run an http broker, applying the bearer-token gate configured via env (open when none is set).
pub async fn run_server<B>(addr: SocketAddr, broker: B) -> Result<(), std::io::Error>
where
    B: Broker,
{
    let listener = TcpListener::bind(addr).await?;
    serve_with_auth(listener, broker, BrokerAuth::from_env().map(Arc::new)).await
}

/// serve without authentication (used by tests and in-process/trusted deployments).
pub async fn serve<B>(listener: TcpListener, broker: B) -> Result<(), std::io::Error>
where
    B: Broker,
{
    serve_with_auth(listener, broker, None).await
}

/// serve with an optional bearer-token gate. `None` leaves every endpoint open.
pub async fn serve_with_auth<B>(
    listener: TcpListener,
    broker: B,
    auth: Option<Arc<BrokerAuth>>,
) -> Result<(), std::io::Error>
where
    B: Broker,
{
    let state = AppState {
        broker: Arc::new(broker),
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/control/publish", post(publish_control::<B>))
        .route("/control/receive", post(receive_control::<B>))
        .route("/control/ack", post(ack_control::<B>))
        .route("/control/nack", post(nack_control::<B>))
        .route("/agent/publish", post(publish_agent::<B>))
        .route("/agent/receive", post(receive_agent::<B>))
        .route("/agent/ack", post(ack_agent::<B>))
        .route("/agent/nack", post(nack_agent::<B>))
        .route("/effects/publish", post(publish_effect::<B>))
        .route("/effects/receive", post(receive_effect::<B>))
        .route(
            "/effects/infrastructure/receive",
            post(receive_infrastructure_effect::<B>),
        )
        .route("/effects/ack", post(ack_effect::<B>))
        .route("/effects/nack", post(nack_effect::<B>))
        .route("/effect-results/publish", post(publish_effect_result::<B>))
        .route("/effect-results/receive", post(receive_effect_result::<B>))
        .route("/effect-results/ack", post(ack_effect_result::<B>))
        .route("/effect-results/nack", post(nack_effect_result::<B>))
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
        .with_state(state)
        .layer(middleware::from_fn_with_state(auth, authenticate));

    axum::serve(listener, app).await
}

// gate every endpoint (except /health) behind the bearer token when auth is configured; attach the
// resolved identity so authz-aware handlers can read it. open (anonymous) when `auth` is `None`.
async fn authenticate(
    State(auth): State<Option<Arc<BrokerAuth>>>,
    mut request: Request,
    next: Next,
) -> Response {
    if request.uri().path() == "/health" {
        request.extensions_mut().insert(AuthIdentity(None));
        return next.run(request).await;
    }
    let identity = match auth.as_deref() {
        None => AuthIdentity(None),
        Some(auth) => match bearer_token(&request).and_then(|token| auth.verify(&token)) {
            Some(claims) => AuthIdentity(Some(claims)),
            None => return unauthorized(),
        },
    };
    request.extensions_mut().insert(identity);
    next.run(request).await
}

fn bearer_token(request: &Request) -> Option<String> {
    request
        .headers()
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(|token| token.trim().to_string())
}

fn unauthorized() -> Response {
    (StatusCode::UNAUTHORIZED, "missing or invalid broker token").into_response()
}

fn forbidden(detail: &str) -> Response {
    (StatusCode::FORBIDDEN, detail.to_string()).into_response()
}

/// authorize an effect or agent receive: a replica-scoped token (`rid`) may only receive for its own
/// replica, closing cross-replica impersonation. an unscoped (plain user) token is allowed;
/// auth-disabled requests carry no constraint.
#[allow(clippy::result_large_err)] // the response is returned directly by the axum handler.
pub(crate) fn authorize_receive(
    identity: &AuthIdentity,
    profile: Option<&ConsumerProfile>,
) -> Result<(), Response> {
    let Some(claims) = &identity.0 else {
        return Ok(());
    };
    match profile.and_then(|profile| profile.replica_id) {
        Some(replica_id) if replica_id.to_string() == claims.rid => Ok(()),
        _ => Err(forbidden("token is scoped to a different replica")),
    }
}

// a replica-scoped token must use the targeted /receive path, not the general-pool /poll drain.
#[allow(clippy::result_large_err)] // the response is returned directly by the axum handler.
async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
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
    let received = match &request.profile {
        Some(profile) => state.broker.receive_control_for(profile).await,
        None => state.broker.receive_control(&request.consumer).await,
    };
    match received {
        Ok(delivery) => json_response(StatusCode::OK, ReceiveControlResponse { delivery }),
        Err(err) => error_response(err),
    }
}

async fn nack_control<B>(
    State(state): State<AppState<B>>,
    Json(request): Json<AckRequest>,
) -> Response
where
    B: Broker,
{
    respond(
        state
            .broker
            .nack_control(&request.consumer, request.delivery_id)
            .await,
        StatusCode::OK,
    )
}

async fn publish_agent<B>(
    State(state): State<AppState<B>>,
    Json(request): Json<PublishAgentRequest>,
) -> Response
where
    B: Broker,
{
    respond(
        state.broker.publish_agent(request.command).await,
        StatusCode::CREATED,
    )
}

async fn receive_agent<B>(
    State(state): State<AppState<B>>,
    Extension(identity): Extension<AuthIdentity>,
    Json(request): Json<ReceiveRequest>,
) -> Response
where
    B: Broker,
{
    if let Err(response) = authorize_receive(&identity, request.profile.as_ref()) {
        return response;
    }
    let received = match &request.profile {
        Some(profile) => state.broker.receive_agent_for(profile).await,
        None => state.broker.receive_agent(&request.consumer).await,
    };
    match received {
        Ok(delivery) => json_response(StatusCode::OK, ReceiveAgentResponse { delivery }),
        Err(err) => error_response(err),
    }
}

async fn ack_agent<B>(State(state): State<AppState<B>>, Json(request): Json<AckRequest>) -> Response
where
    B: Broker,
{
    respond(
        state
            .broker
            .ack_agent(&request.consumer, request.delivery_id)
            .await,
        StatusCode::OK,
    )
}

async fn nack_agent<B>(
    State(state): State<AppState<B>>,
    Json(request): Json<AckRequest>,
) -> Response
where
    B: Broker,
{
    respond(
        state
            .broker
            .nack_agent(&request.consumer, request.delivery_id)
            .await,
        StatusCode::OK,
    )
}

async fn publish_effect<B>(
    State(state): State<AppState<B>>,
    Json(request): Json<PublishEffectRequest>,
) -> Response
where
    B: Broker,
{
    respond(
        state.broker.publish_effect(request.message).await,
        StatusCode::CREATED,
    )
}

async fn receive_effect<B>(
    State(state): State<AppState<B>>,
    Extension(identity): Extension<AuthIdentity>,
    Json(request): Json<ReceiveRequest>,
) -> Response
where
    B: Broker,
{
    if let Err(response) = authorize_receive(&identity, request.profile.as_ref()) {
        return response;
    }
    let received = match &request.profile {
        Some(profile) => state.broker.receive_effect_for(profile).await,
        None => state.broker.receive_effect(&request.consumer).await,
    };
    match received {
        Ok(delivery) => json_response(StatusCode::OK, ReceiveEffectResponse { delivery }),
        Err(error) => error_response(error),
    }
}

async fn receive_infrastructure_effect<B>(
    State(state): State<AppState<B>>,
    Json(request): Json<ReceiveRequest>,
) -> Response
where
    B: Broker,
{
    match state
        .broker
        .receive_infrastructure_effect(&request.consumer)
        .await
    {
        Ok(delivery) => json_response(StatusCode::OK, ReceiveEffectResponse { delivery }),
        Err(error) => error_response(error),
    }
}

async fn ack_effect<B>(
    State(state): State<AppState<B>>,
    Json(request): Json<AckRequest>,
) -> Response
where
    B: Broker,
{
    respond(
        state
            .broker
            .ack_effect(&request.consumer, request.delivery_id)
            .await,
        StatusCode::OK,
    )
}

async fn nack_effect<B>(
    State(state): State<AppState<B>>,
    Json(request): Json<AckRequest>,
) -> Response
where
    B: Broker,
{
    respond(
        state
            .broker
            .nack_effect(&request.consumer, request.delivery_id)
            .await,
        StatusCode::OK,
    )
}

async fn publish_effect_result<B>(
    State(state): State<AppState<B>>,
    Json(request): Json<PublishEffectResultRequest>,
) -> Response
where
    B: Broker,
{
    respond(
        state.broker.publish_effect_result(request.message).await,
        StatusCode::CREATED,
    )
}

async fn receive_effect_result<B>(
    State(state): State<AppState<B>>,
    Json(request): Json<ReceiveRequest>,
) -> Response
where
    B: Broker,
{
    match state.broker.receive_effect_result(&request.consumer).await {
        Ok(delivery) => json_response(StatusCode::OK, ReceiveEffectResultResponse { delivery }),
        Err(error) => error_response(error),
    }
}

async fn ack_effect_result<B>(
    State(state): State<AppState<B>>,
    Json(request): Json<AckRequest>,
) -> Response
where
    B: Broker,
{
    respond(
        state
            .broker
            .ack_effect_result(&request.consumer, request.delivery_id)
            .await,
        StatusCode::OK,
    )
}

async fn nack_effect_result<B>(
    State(state): State<AppState<B>>,
    Json(request): Json<AckRequest>,
) -> Response
where
    B: Broker,
{
    respond(
        state
            .broker
            .nack_effect_result(&request.consumer, request.delivery_id)
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
        BrokerError::WorkflowEffectsUnsupported(message) => json_response(
            StatusCode::NOT_IMPLEMENTED,
            ErrorResponse::new("workflow_results_unsupported", message),
        ),
        BrokerError::Internal(message) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            ErrorResponse::new("internal", message),
        ),
        BrokerError::FeatureDisabled(feature) => json_response(
            StatusCode::NOT_IMPLEMENTED,
            ErrorResponse::new("feature_disabled", feature),
        ),
        BrokerError::ConsumerStreamEnded => json_response(
            StatusCode::SERVICE_UNAVAILABLE,
            ErrorResponse::new("consumer_stream_ended", "consumer stream ended"),
        ),
        // an http broker server does not authenticate against anything upstream, so this can only
        // arrive having been surfaced by a backend it wraps. mirror the status the client would
        // have seen, so a caller can tell "your credential is bad" from "try again later".
        BrokerError::Unauthorized(message) => json_response(
            StatusCode::UNAUTHORIZED,
            ErrorResponse::new("unauthorized", message),
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
