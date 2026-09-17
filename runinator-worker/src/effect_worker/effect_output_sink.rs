#[allow(unused_imports)]
use super::*;

pub(super) struct EffectOutputSink {
    pub(super) command: runinator_comm::EffectCommand,
    pub(super) broker: Arc<dyn Broker>,
    pub(super) uploader: Arc<crate::artifact_upload::ArtifactUploader>,
    pub(super) outbox: Arc<dyn crate::agent::outbox::ResultOutbox>,
    pub(super) events: Arc<dyn crate::events::WorkerEventSink>,
    pub(super) handle: tokio::runtime::Handle,
    pub(super) pending: StdMutex<Vec<tokio::task::JoinHandle<Result<(), SendableError>>>>,
    pub(super) ai_usage: RetainedAiUsage,
    pub(super) publish_order: Arc<tokio::sync::Mutex<()>>,
    pub(super) terminal: StdMutex<Option<Receiver<ProviderTerminalControl>>>,
}

impl EffectOutputSink {
    pub(super) fn new(
        command: runinator_comm::EffectCommand,
        broker: Arc<dyn Broker>,
        api_client: AsyncApiClient<StaticLocator>,
        outbox: Arc<dyn crate::agent::outbox::ResultOutbox>,
        events: Arc<dyn crate::events::WorkerEventSink>,
        terminal: Receiver<ProviderTerminalControl>,
    ) -> Self {
        Self {
            command,
            broker,
            uploader: crate::artifact_upload::ArtifactUploader::new(api_client),
            outbox,
            events,
            handle: tokio::runtime::Handle::current(),
            pending: StdMutex::new(Vec::new()),
            ai_usage: RetainedAiUsage::default(),
            publish_order: Arc::new(tokio::sync::Mutex::new(())),
            terminal: StdMutex::new(Some(terminal)),
        }
    }

    pub(super) fn spawn(
        &self,
        task: impl std::future::Future<Output = Result<(), SendableError>> + Send + 'static,
    ) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.push(self.handle.spawn(task));
        }
    }

    pub(super) async fn flush(&self) -> Result<(), SendableError> {
        let pending = self
            .pending
            .lock()
            .map(|mut pending| std::mem::take(&mut *pending))
            .unwrap_or_default();
        for task in pending {
            task.await
                .map_err(|error| -> SendableError { Box::new(error) })??;
        }
        Ok(())
    }

    pub(super) fn take_ai_usage(&self) -> Option<runinator_models::ai_usage::AiUsage> {
        self.ai_usage.take()
    }
}

impl ProviderEventSink for EffectOutputSink {
    fn take_terminal_control(&self) -> Option<Receiver<ProviderTerminalControl>> {
        self.terminal.lock().ok()?.take()
    }

    fn emit(&self, event: runinator_models::runs::ProviderExecutionEvent) {
        match event {
            runinator_models::runs::ProviderExecutionEvent::Chunk { stream, content } => {
                self.events
                    .handle(crate::events::WorkerEvent::EffectOutputChunk {
                        workflow_run_id: self.command.workflow_run_id,
                        effect_id: self.command.effect_id,
                        stream: stream.clone(),
                        content: content.clone(),
                    });
                let broker = self.broker.clone();
                let command = self.command.clone();
                let outbox = self.outbox.clone();
                let publish_order = self.publish_order.clone();
                self.spawn(async move {
                    // PTY bytes and ordinary stdout/stderr chunks must reach durable storage in
                    // emission order. Concurrent broker publishes can otherwise scramble ANSI
                    // cursor sequences or adjacent log lines.
                    let _ordered = publish_order.lock().await;
                    let mut result = EffectResult {
                        workspace_commit: None,
                        version: command.version,
                        event_id: Uuid::now_v7(),
                        effect_id: command.effect_id,
                        workflow_run_id: command.workflow_run_id,
                        continuation_id: command.continuation_id,
                        attempt: command.attempt,
                        ai_usage: None,
                        kind: EffectResultKind::Chunk { stream, content },
                        timestamp: chrono::Utc::now(),
                        trace_id: command.trace_id,
                        notification_delivery_id: command.notification_delivery_id,
                    };
                    publish_result(broker.as_ref(), outbox.as_ref(), &mut result, false).await
                });
            }
            runinator_models::runs::ProviderExecutionEvent::Artifact {
                name,
                mime_type,
                size_bytes,
                uri,
                metadata,
            } => {
                let broker = self.broker.clone();
                let command = self.command.clone();
                let uploader = self.uploader.clone();
                let outbox = self.outbox.clone();
                self.spawn(async move {
                    let mut artifact = NewRunArtifact {
                        name,
                        mime_type,
                        size_bytes,
                        uri,
                        metadata,
                    };
                    uploader.relocate_effect(&command, &mut artifact).await;
                    let mut result = EffectResult {
                        workspace_commit: None,
                        version: command.version,
                        event_id: Uuid::now_v7(),
                        effect_id: command.effect_id,
                        workflow_run_id: command.workflow_run_id,
                        continuation_id: command.continuation_id,
                        attempt: command.attempt,
                        ai_usage: None,
                        kind: EffectResultKind::Artifact {
                            artifact: Value::encode(&artifact)?,
                        },
                        timestamp: chrono::Utc::now(),
                        trace_id: command.trace_id,
                        notification_delivery_id: command.notification_delivery_id,
                    };
                    publish_result(broker.as_ref(), outbox.as_ref(), &mut result, true).await
                });
            }
            runinator_models::runs::ProviderExecutionEvent::Progress { kind, payload } => {
                let broker = self.broker.clone();
                let command = self.command.clone();
                let outbox = self.outbox.clone();
                let publish_order = self.publish_order.clone();
                self.spawn(async move {
                    let _ordered = publish_order.lock().await;
                    let mut result = EffectResult {
                        workspace_commit: None,
                        version: command.version,
                        event_id: Uuid::now_v7(),
                        effect_id: command.effect_id,
                        workflow_run_id: command.workflow_run_id,
                        continuation_id: command.continuation_id,
                        attempt: command.attempt,
                        ai_usage: None,
                        kind: EffectResultKind::Progress { kind, payload },
                        timestamp: chrono::Utc::now(),
                        trace_id: command.trace_id,
                        notification_delivery_id: command.notification_delivery_id,
                    };
                    publish_result(broker.as_ref(), outbox.as_ref(), &mut result, false).await
                });
            }
            runinator_models::runs::ProviderExecutionEvent::TerminalInteraction { interaction } => {
                let broker = self.broker.clone();
                let command = self.command.clone();
                let outbox = self.outbox.clone();
                let publish_order = self.publish_order.clone();
                self.spawn(async move {
                    let _ordered = publish_order.lock().await;
                    let mut result = EffectResult {
                        workspace_commit: None,
                        version: command.version,
                        event_id: Uuid::now_v7(),
                        effect_id: command.effect_id,
                        workflow_run_id: command.workflow_run_id,
                        continuation_id: command.continuation_id,
                        attempt: command.attempt,
                        ai_usage: None,
                        kind: EffectResultKind::TerminalInteraction { interaction },
                        timestamp: chrono::Utc::now(),
                        trace_id: command.trace_id,
                        notification_delivery_id: command.notification_delivery_id,
                    };
                    publish_result(broker.as_ref(), outbox.as_ref(), &mut result, true).await
                });
            }
            runinator_models::runs::ProviderExecutionEvent::AiUsage { usage } => {
                self.ai_usage.retain(usage);
            }
            runinator_models::runs::ProviderExecutionEvent::Message { .. } => {}
        }
    }
}
