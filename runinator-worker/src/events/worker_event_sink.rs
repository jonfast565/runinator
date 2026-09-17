#[allow(unused_imports)]
use super::*;

pub trait WorkerEventSink: Send + Sync {
    fn handle(&self, event: WorkerEvent);
}

impl<F> WorkerEventSink for F
where
    F: Fn(WorkerEvent) + Send + Sync,
{
    fn handle(&self, event: WorkerEvent) {
        self(event)
    }
}
