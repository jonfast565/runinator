use std::sync::Arc;
use std::time::Duration;

use runinator_broker::Broker;
use runinator_models::errors::error_code_or_unknown;
use tokio::sync::{Notify, broadcast};
use tracing::{error, info};
use uuid::Uuid;

use crate::events::AppEvent;

// how long the event consumer waits before retrying after a broker receive error.
const EVENT_CONSUMER_RETRY_BACKOFF: Duration = Duration::from_millis(250);

/// consume the broker fan-out events channel and re-broadcast every event to this replica's local
/// WebSocket clients. each replica subscribes with its own per-replica `instance_id`, so every
/// replica receives every UI event regardless of which replica emitted it (including events from a
/// standalone background worker).
pub(crate) async fn run_event_consumer(
    broker: Arc<dyn Broker>,
    local: broadcast::Sender<AppEvent>,
    instance_id: String,
    shutdown: Arc<Notify>,
) {
    info!("event consumer started");
    loop {
        let received = tokio::select! {
            _ = shutdown.notified() => {
                info!("event consumer shutting down");
                return;
            }
            received = broker.receive_event(&instance_id) => received,
        };
        match received {
            // a send error just means no WebSocket clients are connected right now; events are
            // best-effort, so drop it.
            Ok(delivery) => {
                let _ = local.send(delivery.event);
            }
            Err(err) => {
                error!(
                    error_code = error_code_or_unknown(&err),
                    "failed to receive UI event: {}", err
                );
                tokio::select! {
                    _ = shutdown.notified() => return,
                    _ = tokio::time::sleep(EVENT_CONSUMER_RETRY_BACKOFF) => {}
                }
            }
        }
    }
}

/// a stable per-process identifier used when claiming database rows and subscribing to broker
/// fan-out channels.
pub(crate) fn instance_id() -> String {
    format!("runinator-ws-{}", Uuid::new_v4())
}
