#[allow(unused_imports)]
use super::*;

pub(crate) struct WebSocketGuard {
    pub(super) kind: &'static str,
}

impl Drop for WebSocketGuard {
    fn drop(&mut self) {
        runinator_tui::gauge_increment("web service", "WebSockets", -1);
        metrics::gauge!(WS_CONNECTIONS, "kind" => self.kind).decrement(1.0);
        metrics::counter!(WS_CONNECTIONS_TOTAL, "kind" => self.kind, "outcome" => "closed")
            .increment(1);
        handles()
            .websocket_connections
            .add(-1, &[KeyValue::new("kind", self.kind)]);
        handles().websocket_connections_total.add(
            1,
            &[
                KeyValue::new("kind", self.kind),
                KeyValue::new("outcome", "closed"),
            ],
        );
    }
}
