use std::{ops::Deref, sync::Arc};

use runinator_broker_core::{Broker, EmbeddedEngineSignals, UiEventPublisher};
use tokio::sync::broadcast;

// The UI event contract crosses the broker fan-out channel, while this module owns only the local
// WebSocket broadcast bridge for one web-service replica.
pub use runinator_broker_core::{
    AppEvent, AppEventKind, emit, emit_pipeline_run, emit_task_run, emit_workflow_run,
    emit_workflows_changed,
};

/// Fan-out bus for UI events in one web-service replica.
///
/// All writes go through the broker-backed [`UiEventPublisher`]; only the web service's broker
/// consumer writes `local`, which feeds this process's WebSocket clients. Optional local signals
/// are latency hints for an engine embedded by this same process and are absent for standalone
/// engine deployments.
#[derive(Clone)]
pub struct EventBus {
    local: broadcast::Sender<AppEvent>,
    publisher: UiEventPublisher,
    local_signals: Option<EmbeddedEngineSignals>,
}

impl EventBus {
    pub fn new(local: broadcast::Sender<AppEvent>, broker: Arc<dyn Broker>) -> Self {
        Self::from_publisher(local, UiEventPublisher::new(broker))
    }

    pub fn from_publisher(local: broadcast::Sender<AppEvent>, publisher: UiEventPublisher) -> Self {
        Self {
            local,
            publisher,
            local_signals: None,
        }
    }

    /// Attach process-local signals only when this web-service process embeds an engine.
    pub fn with_embedded_engine_signals(mut self, signals: Option<EmbeddedEngineSignals>) -> Self {
        self.local_signals = signals;
        self
    }

    /// Subscribe a WebSocket client to this replica's locally-broadcast events.
    pub fn subscribe(&self) -> broadcast::Receiver<AppEvent> {
        self.local.subscribe()
    }

    /// Clone the transport-backed publisher for an application service. The service still has no
    /// access to this replica's WebSocket broadcast receiver.
    pub fn publisher(&self) -> UiEventPublisher {
        self.publisher.clone()
    }

    /// Clone the optional local latency hints for an application service. Their absence is the
    /// normal standalone-engine deployment shape, where durable polling remains authoritative.
    pub fn embedded_engine_signals(&self) -> Option<EmbeddedEngineSignals> {
        self.local_signals.clone()
    }

    /// Prompt the embedded VM driver to poll its durable continuation queue. This is a no-op when
    /// the engine runs out of process, where the same durable queue is reached by normal polling.
    pub fn nudge_workflow_vm(&self) {
        if let Some(signals) = &self.local_signals {
            signals.nudge_workflow_vm();
        }
    }

    /// Prompt the embedded agent-directive publisher to poll its durable outbox.
    pub fn nudge_agent_directives(&self) {
        if let Some(signals) = &self.local_signals {
            signals.nudge_agent_directives();
        }
    }
}

impl Deref for EventBus {
    type Target = UiEventPublisher;

    fn deref(&self) -> &Self::Target {
        &self.publisher
    }
}

// The threaded handle stays named EventSender so handler signatures stay focused on the event role.
pub type EventSender = EventBus;

pub fn nudge_workflow_vm(events: &EventSender) {
    events.nudge_workflow_vm();
}

pub fn nudge_agent_directives(events: &EventSender) {
    events.nudge_agent_directives();
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use runinator_broker_core::in_memory::InMemoryBroker;

    use super::*;

    #[tokio::test]
    async fn bus_forwards_local_nudges_to_its_embedded_engine_signals() {
        let (local, _) = broadcast::channel(1);
        let signals = EmbeddedEngineSignals::new();
        let bus = EventBus::new(local, Arc::new(InMemoryBroker::new()))
            .with_embedded_engine_signals(Some(signals.clone()));

        nudge_workflow_vm(&bus);
        nudge_agent_directives(&bus);

        tokio::time::timeout(
            Duration::from_secs(1),
            signals.workflow_vm_notifier().notified(),
        )
        .await
        .expect("workflow VM nudge should reach the embedded engine");
        tokio::time::timeout(
            Duration::from_secs(1),
            signals.agent_directives_notifier().notified(),
        )
        .await
        .expect("agent directive nudge should reach the embedded engine");
    }
}
