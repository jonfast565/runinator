use std::sync::{Arc, Mutex};

use log::error;
use runinator_api::{AsyncApiClient, RunArtifactPayload, RunChunkPayload, StaticLocator};
use runinator_models::runs::{ProviderExecutionEvent, TaskExecutionResult};
use runinator_plugin::provider::ProviderEventSink;
use serde_json::Value;
use tokio::runtime::Handle;

#[derive(Clone)]
pub struct RunOutputSink {
    run_id: Option<i64>,
    api_client: AsyncApiClient<StaticLocator>,
    handle: Handle,
    state: Arc<Mutex<RunOutputState>>,
}

#[derive(Default)]
struct RunOutputState {
    message: Option<String>,
}

impl RunOutputSink {
    pub fn new(
        run_id: Option<i64>,
        api_client: AsyncApiClient<StaticLocator>,
        handle: Handle,
    ) -> Self {
        Self {
            run_id,
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
        let Some(run_id) = self.run_id else {
            return;
        };

        for chunk in &result.chunks {
            if let Err(err) = self
                .api_client
                .append_run_chunk(
                    run_id,
                    &RunChunkPayload {
                        stream: chunk.stream.clone(),
                        content: chunk.content.clone(),
                    },
                )
                .await
            {
                error!("Failed to append run {} result chunk: {}", run_id, err);
            }
        }

        for artifact in &result.artifacts {
            if let Err(err) = self
                .api_client
                .add_run_artifact(
                    run_id,
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
                error!("Failed to add run {} result artifact: {}", run_id, err);
            }
        }
    }

    fn emit_chunk(&self, stream: String, content: String) {
        let Some(run_id) = self.run_id else {
            return;
        };
        let client = self.api_client.clone();
        self.handle.spawn(async move {
            let payload = RunChunkPayload { stream, content };
            if let Err(err) = client.append_run_chunk(run_id, &payload).await {
                error!("Failed to append run {} streamed chunk: {}", run_id, err);
            }
        });
    }

    fn emit_artifact(
        &self,
        name: String,
        mime_type: String,
        size_bytes: i64,
        uri: String,
        metadata: Value,
    ) {
        let Some(run_id) = self.run_id else {
            return;
        };
        let client = self.api_client.clone();
        self.handle.spawn(async move {
            let payload = RunArtifactPayload {
                name,
                mime_type,
                size_bytes,
                uri,
                metadata,
            };
            if let Err(err) = client.add_run_artifact(run_id, &payload).await {
                error!("Failed to add run {} streamed artifact: {}", run_id, err);
            }
        });
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
