#[allow(unused_imports)]
use super::*;

pub struct NoopEventSink;

impl WorkerEventSink for NoopEventSink {
    fn handle(&self, _event: WorkerEvent) {}
}
