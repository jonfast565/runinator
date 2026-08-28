//! constrained agent-directive execution.

use std::{future::Future, pin::Pin, sync::Arc, time::Duration};

use chrono::Utc;
use runinator_broker::{AgentDelivery, Broker, IngressMessage};
use runinator_comm::{
    AgentDirectiveKind, AgentDirectiveResult, AgentDirectiveStatus, ConsumerProfile,
    WsIngressCommand,
};
use runinator_models::{errors::SendableError, value::Value};
use tokio::sync::Notify;
use tracing::{error, info};

const RECEIVE_RETRY_BACKOFF: Duration = Duration::from_secs(1);

/// result supplied by a host-specific directive handler.
pub struct DirectiveResponse {
    pub status: AgentDirectiveStatus,
    pub payload: Value,
    pub message: Option<String>,
}

impl DirectiveResponse {
    pub fn completed(payload: Value) -> Self {
        Self {
            status: AgentDirectiveStatus::Completed,
            payload,
            message: None,
        }
    }

    pub fn unsupported(message: impl Into<String>) -> Self {
        Self {
            status: AgentDirectiveStatus::Unsupported,
            payload: Value::Null,
            message: Some(message.into()),
        }
    }

    pub fn failed(message: impl Into<String>) -> Self {
        Self {
            status: AgentDirectiveStatus::Failed,
            payload: Value::Null,
            message: Some(message.into()),
        }
    }
}

/// host seam for directives that require desktop-only state such as the sandbox and log ring.
pub trait DirectiveHandler: Send + Sync + 'static {
    fn handle<'a>(
        &'a self,
        kind: &'a AgentDirectiveKind,
    ) -> Pin<Box<dyn Future<Output = DirectiveResponse> + Send + 'a>>;
}

/// fail-closed handler used by the generic/headless runtime.
#[derive(Default)]
pub struct DefaultDirectiveHandler;

impl DirectiveHandler for DefaultDirectiveHandler {
    fn handle<'a>(
        &'a self,
        kind: &'a AgentDirectiveKind,
    ) -> Pin<Box<dyn Future<Output = DirectiveResponse> + Send + 'a>> {
        Box::pin(async move {
            DirectiveResponse::unsupported(format!(
                "{} is unavailable on this agent host",
                directive_name(kind)
            ))
        })
    }
}

pub(crate) struct DirectiveLoopState {
    pub drained: Arc<std::sync::atomic::AtomicBool>,
    pub restart_requested: Arc<std::sync::atomic::AtomicBool>,
    pub state_changed: Arc<Notify>,
}

pub(crate) async fn run_directive_loop(
    broker: Arc<dyn Broker>,
    profile: ConsumerProfile,
    handler: Arc<dyn DirectiveHandler>,
    state: DirectiveLoopState,
    shutdown: Arc<Notify>,
) -> Result<(), SendableError> {
    let consumer_id = profile.id.clone();
    loop {
        let delivery = tokio::select! {
            _ = shutdown.notified() => return Ok(()),
            received = broker.receive_agent_for(&profile) => match received {
                Ok(delivery) => delivery,
                Err(err) => {
                    error!("failed to receive agent directive: {err}");
                    tokio::select! {
                        _ = shutdown.notified() => return Ok(()),
                        _ = tokio::time::sleep(RECEIVE_RETRY_BACKOFF) => {}
                    }
                    continue;
                }
            }
        };
        handle_delivery(&broker, &consumer_id, handler.as_ref(), &state, delivery).await?;
    }
}

async fn handle_delivery(
    broker: &Arc<dyn Broker>,
    consumer_id: &str,
    handler: &dyn DirectiveHandler,
    state: &DirectiveLoopState,
    delivery: AgentDelivery,
) -> Result<(), SendableError> {
    let command = delivery.command;
    let mut restart_after_ack = false;
    let response = if command.expires_at <= Utc::now() {
        DirectiveResponse::failed("directive expired before delivery")
    } else {
        match &command.kind {
            AgentDirectiveKind::Diagnostics => {
                DirectiveResponse::completed(runinator_models::json!({
                    "pid": std::process::id(),
                    "os": std::env::consts::OS,
                    "arch": std::env::consts::ARCH,
                    "version": env!("CARGO_PKG_VERSION"),
                    "drained": state.drained.load(std::sync::atomic::Ordering::SeqCst),
                }))
            }
            AgentDirectiveKind::Drain => {
                state
                    .drained
                    .store(true, std::sync::atomic::Ordering::SeqCst);
                state.state_changed.notify_one();
                DirectiveResponse::completed(runinator_models::json!({ "drained": true }))
            }
            AgentDirectiveKind::Undrain => {
                state
                    .drained
                    .store(false, std::sync::atomic::Ordering::SeqCst);
                state.state_changed.notify_one();
                DirectiveResponse::completed(runinator_models::json!({ "drained": false }))
            }
            AgentDirectiveKind::Restart => {
                restart_after_ack = true;
                DirectiveResponse::completed(runinator_models::json!({ "restarting": true }))
            }
            AgentDirectiveKind::SetLogLevel { level } => DirectiveResponse::completed(
                runinator_models::json!({ "level": level, "applies_on_restart": true }),
            ),
            AgentDirectiveKind::CleanupWorkspace {
                workspace_id,
                local_key,
            } => cleanup_workspace(*workspace_id, local_key).await,
            AgentDirectiveKind::SetLabels { .. }
            | AgentDirectiveKind::SetConcurrency { .. }
            | AgentDirectiveKind::RepublishProviders
            | AgentDirectiveKind::RotateCredential => DirectiveResponse::unsupported(
                "this setting cannot be changed safely by the running process",
            ),
            AgentDirectiveKind::Unknown => {
                DirectiveResponse::unsupported("directive type is unknown to this agent version")
            }
            other => handler.handle(other).await,
        }
    };
    let result = AgentDirectiveResult {
        directive_id: command.directive_id,
        status: response.status,
        payload: response.payload,
        message: response.message,
    };
    let ingress = WsIngressCommand::AgentDirectiveResult { result };
    let message = IngressMessage {
        dedupe_key: Some(ingress.dedupe_key()),
        command: ingress,
        enqueued_at: Utc::now(),
    };
    if let Err(err) = broker.publish_ingress(message).await {
        broker
            .nack_agent(consumer_id, delivery.delivery_id)
            .await
            .map_err(|nack| Box::new(nack) as SendableError)?;
        return Err(Box::new(err));
    }
    broker
        .ack_agent(consumer_id, delivery.delivery_id)
        .await
        .map_err(|err| Box::new(err) as SendableError)?;
    info!(directive_id = %command.directive_id, "agent directive completed");
    if restart_after_ack {
        state
            .restart_requested
            .store(true, std::sync::atomic::Ordering::SeqCst);
        state.state_changed.notify_one();
    }
    Ok(())
}

async fn cleanup_workspace(workspace_id: uuid::Uuid, local_key: &str) -> DirectiveResponse {
    let path = std::path::Path::new(local_key);
    if local_key.trim().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return DirectiveResponse::failed("workspace key is not a safe relative path");
    }
    let Some(root) = std::env::var_os("RUNINATOR_WORKSPACE_ROOT") else {
        return DirectiveResponse::failed("worker workspace root is not configured");
    };
    let root = match tokio::fs::canonicalize(root).await {
        Ok(root) => root,
        Err(error) => {
            return DirectiveResponse::failed(format!("workspace root is unavailable: {error}"));
        }
    };
    let target = root.join(path);
    if !target.starts_with(&root) {
        return DirectiveResponse::failed("workspace key escapes the configured root");
    }
    match tokio::fs::remove_dir_all(&target).await {
        Ok(()) => DirectiveResponse::completed(runinator_models::json!({
            "workspace_id": workspace_id,
            "removed": true,
        })),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            DirectiveResponse::completed(runinator_models::json!({
                "workspace_id": workspace_id,
                "removed": false,
                "already_absent": true,
            }))
        }
        Err(error) => DirectiveResponse::failed(format!("workspace cleanup failed: {error}")),
    }
}

fn directive_name(kind: &AgentDirectiveKind) -> &'static str {
    match kind {
        AgentDirectiveKind::Diagnostics => "diagnostics",
        AgentDirectiveKind::TailLogs { .. } => "tail_logs",
        AgentDirectiveKind::ListSandbox { .. } => "list_sandbox",
        AgentDirectiveKind::FetchFile { .. } => "fetch_file",
        AgentDirectiveKind::CleanupWorkspace { .. } => "cleanup_workspace",
        AgentDirectiveKind::SetLabels { .. } => "set_labels",
        AgentDirectiveKind::SetConcurrency { .. } => "set_concurrency",
        AgentDirectiveKind::SetLogLevel { .. } => "set_log_level",
        AgentDirectiveKind::RepublishProviders => "republish_providers",
        AgentDirectiveKind::Drain => "drain",
        AgentDirectiveKind::Undrain => "undrain",
        AgentDirectiveKind::Restart => "restart",
        AgentDirectiveKind::RotateCredential => "rotate_credential",
        AgentDirectiveKind::Unknown => "unknown",
    }
}
