#[allow(unused_imports)]
use super::*;

pub(super) struct WsMetrics {
    pub(super) requests: Counter<u64>,
    pub(super) duration_ms: Histogram<f64>,
    pub(super) in_flight: UpDownCounter<i64>,
    pub(super) rejections: Counter<u64>,
    pub(super) websocket_connections: UpDownCounter<i64>,
    pub(super) websocket_connections_total: Counter<u64>,
}
