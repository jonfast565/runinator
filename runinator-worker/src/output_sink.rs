use std::sync::{Arc, Mutex};

use log::error;
use runinator_api::{AsyncApiClient, RunArtifactPayload, RunChunkPayload, StaticLocator};
use runinator_models::runs::{ProviderExecutionEvent, TaskExecutionResult};
use runinator_plugin::provider::ProviderEventSink;
use serde_json::Value;
use tokio::{runtime::Handle, task::JoinHandle};

#[derive(Clone)]
pub struct RunOutputSink {
    workflow_node_run_id: i64,
    api_client: AsyncApiClient<StaticLocator>,
    handle: Handle,
    state: Arc<Mutex<RunOutputState>>,
}

#[derive(Default)]
struct RunOutputState {
    message: Option<String>,
    pending: Vec<JoinHandle<()>>,
}

impl RunOutputSink {
    pub fn new(
        workflow_node_run_id: i64,
        api_client: AsyncApiClient<StaticLocator>,
        handle: Handle,
    ) -> Self {
        Self {
            workflow_node_run_id,
            api_client,
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

    pub async fn persist_result(&self, result: &TaskExecutionResult) {
        // only persist artifacts; chunks are streamed via events.jsonl and would otherwise duplicate.
        for artifact in &result.artifacts {
            if let Err(err) = self
                .api_client
                .add_workflow_node_run_artifact(
                    self.workflow_node_run_id,
                    &RunArtifactPayload {
                        name: artifact.name.clone(),
                        mime_type: artifact.mime_type.clone(),
                        size_bytes: artifact.size_bytes,
                        uri: artifact.uri.clone(),
                        metadata: artifact.metadata.clone(),
                    },
                )
                .await
            {
                error!(
                    "Failed to add workflow node run {} result artifact: {}",
                    self.workflow_node_run_id, err
                );
            }
        }
    }

    pub fn emit_log(&self, content: String) {
        self.emit_chunk("log".into(), content);
    }

    pub async fn flush(&self) {
        let pending = self
            .state
            .lock()
            .map(|mut state| std::mem::take(&mut state.pending))
            .unwrap_or_default();
        for handle in pending {
            if let Err(err) = handle.await {
                error!(
                    "Failed to join workflow node run {} output task: {}",
                    self.workflow_node_run_id, err
                );
            }
        }
    }

    fn emit_chunk(&self, stream: String, content: String) {
        let node_run_id = self.workflow_node_run_id;
        let client = self.api_client.clone();
        let handle = self.handle.spawn(async move {
            let payload = RunChunkPayload { stream, content };
            if let Err(err) = client
                .append_workflow_node_run_chunk(node_run_id, &payload)
                .await
            {
                error!(
                    "Failed to append workflow node run {} streamed chunk: {}",
                    node_run_id, err
                );
            }
        });
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
        let node_run_id = self.workflow_node_run_id;
        let client = self.api_client.clone();
        let handle = self.handle.spawn(async move {
            let payload = RunArtifactPayload {
                name,
                mime_type,
                size_bytes,
                uri,
                metadata,
            };
            if let Err(err) = client
                .add_workflow_node_run_artifact(node_run_id, &payload)
                .await
            {
                error!(
                    "Failed to add workflow node run {} streamed artifact: {}",
                    node_run_id, err
                );
            }
        });
        self.track_pending(handle);
    }

    fn track_pending(&self, handle: JoinHandle<()>) {
        if let Ok(mut state) = self.state.lock() {
            state.pending.push(handle);
        }
    }
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
