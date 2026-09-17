#[allow(unused_imports)]
use super::*;

/// A reusable, broker-backed publisher for best-effort UI events.
///
/// The publisher owns no web-service broadcast state and no engine-loop controls, so an embedded
/// web service and a standalone engine worker can share exactly the same event path.
#[derive(Clone)]
pub struct UiEventPublisher {
    pub(super) broker: Arc<dyn Broker>,
}

impl UiEventPublisher {
    pub fn new(broker: Arc<dyn Broker>) -> Self {
        Self { broker }
    }

    /// Publish without delaying the caller's durable operation. A failed UI hint is logged, while
    /// the durable state change that caused it remains the source of truth for a later resync.
    pub fn emit(&self, event: UiEvent) {
        let broker = self.broker.clone();
        tokio::spawn(async move {
            if let Err(err) = broker.publish_event(EventMessage::new(event)).await {
                log::warn!("failed to publish UI event: {err}");
            }
        });
    }
}
