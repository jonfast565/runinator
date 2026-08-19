use std::sync::{Arc, Mutex};

use chrono::Utc;
use runinator_broker::{Broker, BrokerError, ResultMessage};
use runinator_comm::{ActionCommand, WorkflowResultEvent, WorkflowResultEventKind};
use runinator_models::value::Value;
use runinator_models::{
    runs::{NewRunArtifact, NewRunChunk, ProviderExecutionEvent, TaskExecutionResult},
    workflows::WorkflowStatus,
};
use runinator_plugin::provider::ProviderEventSink;
use tokio::{runtime::Handle, task::JoinHandle};
use tracing::{Instrument, error};

use crate::agent::outbox::{OutboxError, ResultOutbox};
use crate::artifact_upload::ArtifactUploader;

#[derive(Clone)]
pub struct RunOutputSink {
    command: ActionCommand,
    broker: Arc<dyn Broker>,
    outbox: Arc<dyn ResultOutbox>,
    /// moves artifact bytes off this worker before their event is published. `None` leaves the
    /// provider's local path in place, which is what an offline or api-less host wants.
    uploader: Option<Arc<ArtifactUploader>>,
    handle: Handle,
    state: Arc<Mutex<RunOutputState>>,
}

#[derive(Default)]
struct RunOutputState {
    message: Option<String>,
    pending: Vec<JoinHandle<()>>,
    errors: Vec<String>,
}

impl RunOutputSink {
    pub fn new(
        command: ActionCommand,
        broker: Arc<dyn Broker>,
        outbox: Arc<dyn ResultOutbox>,
        uploader: Option<Arc<ArtifactUploader>>,
        handle: Handle,
    ) -> Self {
        Self {
            command,
            broker,
            outbox,
            uploader,
            handle,
            state: Arc::new(Mutex::new(RunOutputState::default())),
        }
    }

    pub fn message(&self) -> Option<String> {
        self.state
            .lock()
            .ok()
            .and_then(|state| state.message.clone())
    }

    pub async fn persist_result(&self, result: &TaskExecutionResult) -> Result<(), BrokerError> {
        // only persist artifacts; chunks are streamed via events.jsonl and would otherwise duplicate.
        for artifact in &result.artifacts {
            let mut artifact = artifact.clone();
            self.relocate(&mut artifact).await;
            self.publish_event(WorkflowResultEvent::artifact(&self.command, artifact))
                .await?;
        }
        Ok(())
    }

    /// move an artifact's bytes somewhere every replica can read before its event goes out.
    async fn relocate(&self, artifact: &mut NewRunArtifact) {
        if let Some(uploader) = &self.uploader {
            uploader.relocate(&self.command, artifact).await;
        }
    }

    pub fn emit_log(&self, content: String) {
        self.emit_chunk("log".into(), content);
    }

    pub async fn flush(&self) -> Result<(), BrokerError> {
        let pending = self
            .state
            .lock()
            .map(|mut state| std::mem::take(&mut state.pending))
            .unwrap_or_default();
        for handle in pending {
            if let Err(err) = handle.await {
                error!(
                    node_id = %self.command.workflow_node_run_id,
                    "failed to join workflow node run output task: {}",
                    err
                );
                return Err(BrokerError::Internal(err.to_string()));
            }
        }

        let errors = self
            .state
            .lock()
            .map(|mut state| std::mem::take(&mut state.errors))
            .unwrap_or_default();
        if !errors.is_empty() {
            return Err(BrokerError::Internal(errors.join("; ")));
        }

        Ok(())
    }

    pub async fn publish_status(
        &self,
        status: WorkflowStatus,
        output_json: Option<Value>,
        message: Option<String>,
    ) -> Result<(), BrokerError> {
        self.publish_event(WorkflowResultEvent::status(
            &self.command,
            status,
            output_json,
            message,
        ))
        .await
    }

    fn emit_chunk(&self, stream: String, content: String) {
        let event = WorkflowResultEvent::chunk(&self.command, NewRunChunk { stream, content });
        let broker = self.broker.clone();
        let node_id = self.command.workflow_node_run_id;
        // spawned onto the tokio handle, so it does not inherit the caller's ambient span; carry it
        // explicitly so a failed publish still logs with trace_id/run_id/node_id attached.
        let span = tracing::Span::current();
        let handle = self.handle.spawn(
            async move {
                if let Err(err) = publish_chunk(broker.as_ref(), event).await {
                    error!(node_id = %node_id, "failed to publish workflow result chunk: {}", err);
                }
            }
            .instrument(span),
        );
        self.track_pending(handle);
    }

    fn emit_artifact(
        &self,
        name: String,
        mime_type: String,
        size_bytes: i64,
        uri: String,
        metadata: Value,
    ) {
        let artifact = NewRunArtifact {
            name,
            mime_type,
            size_bytes,
            uri,
            metadata,
        };
        let broker = self.broker.clone();
        let outbox = self.outbox.clone();
        let uploader = self.uploader.clone();
        let command = self.command.clone();
        let state = self.state.clone();
        let node_id = self.command.workflow_node_run_id;
        let span = tracing::Span::current();
        let handle = self.handle.spawn(
            async move {
                // relocation happens inside the spawned task so a slow upload does not block the
                // provider that emitted the artifact.
                let mut artifact = artifact;
                if let Some(uploader) = &uploader {
                    uploader.relocate(&command, &mut artifact).await;
                }
                let event = WorkflowResultEvent::artifact(&command, artifact);
                if let Err(err) = publish_event(broker.as_ref(), outbox.as_ref(), event).await {
                    error!(node_id = %node_id, "failed to publish workflow result artifact: {}", err);
                    if let Ok(mut state) = state.lock() {
                        state.errors.push(err.to_string());
                    }
                }
            }
            .instrument(span),
        );
        self.track_pending(handle);
    }

    fn track_pending(&self, handle: JoinHandle<()>) {
        if let Ok(mut state) = self.state.lock() {
            state.pending.push(handle);
        }
    }

    async fn publish_event(&self, event: WorkflowResultEvent) -> Result<(), BrokerError> {
        publish_event(self.broker.as_ref(), self.outbox.as_ref(), event).await
    }
}

async fn publish_event(
    broker: &dyn Broker,
    outbox: &dyn ResultOutbox,
    event: WorkflowResultEvent,
) -> Result<(), BrokerError> {
    let bufferable = should_buffer(&event);
    let message = ResultMessage {
        dedupe_key: Some(event.event_id.to_string()),
        event,
        enqueued_at: Utc::now(),
    };
    // preserve event order after the first outage: once an artifact is queued, later terminal
    // statuses join it instead of jumping ahead through a recovered broker connection.
    if bufferable && outbox.depth() > 0 {
        let result = append_outbox(outbox, message);
        crate::metrics::result_publish(if result.is_ok() { "buffered" } else { "error" });
        return result;
    }
    match broker.publish_result(message.clone()).await {
        Ok(()) => {
            crate::metrics::result_publish("published");
            Ok(())
        }
        Err(_broker_error) if bufferable => {
            let result = append_outbox(outbox, message);
            crate::metrics::result_publish(if result.is_ok() { "buffered" } else { "error" });
            result
        }
        Err(err) => {
            crate::metrics::result_publish("error");
            Err(err)
        }
    }
}

fn should_buffer(event: &WorkflowResultEvent) -> bool {
    match &event.kind {
        WorkflowResultEventKind::Artifact { .. } => true,
        WorkflowResultEventKind::Status { status, .. } => status.is_terminal(),
        WorkflowResultEventKind::Chunk { .. } => false,
    }
}

/// chunks are deliberately best effort: buffering multi-day logs is a disk-fill vector. broker
/// backends already bound their in-memory write queues, and overflow is dropped rather than making
/// a completed action re-execute merely because a log line could not be delivered.
async fn publish_chunk(broker: &dyn Broker, event: WorkflowResultEvent) -> Result<(), BrokerError> {
    let result = broker
        .publish_result(ResultMessage {
            dedupe_key: Some(event.event_id.to_string()),
            event,
            enqueued_at: Utc::now(),
        })
        .await;
    crate::metrics::result_publish(if result.is_ok() { "published" } else { "error" });
    result
}

fn append_outbox(outbox: &dyn ResultOutbox, message: ResultMessage) -> Result<(), BrokerError> {
    outbox.append(message).map_err(|err| match err {
        OutboxError::Disabled => BrokerError::Internal("result broker unavailable".to_string()),
        other => BrokerError::Internal(other.to_string()),
    })
}

impl ProviderEventSink for RunOutputSink {
    fn emit(&self, event: ProviderExecutionEvent) {
        match event {
            ProviderExecutionEvent::Chunk { stream, content } => self.emit_chunk(stream, content),
            ProviderExecutionEvent::Artifact {
                name,
                mime_type,
                size_bytes,
                uri,
                metadata,
            } => self.emit_artifact(name, mime_type, size_bytes, uri, metadata),
            ProviderExecutionEvent::Message { message } => {
                if let Ok(mut state) = self.state.lock() {
                    state.message = Some(message);
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "output_sink_tests.rs"]
mod tests;
