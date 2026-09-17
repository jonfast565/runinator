#[allow(unused_imports)]
use super::*;

#[derive(Default)]
pub(super) struct MemorySink {
    pub(super) events: Mutex<Vec<ProviderExecutionEvent>>,
}

impl ProviderEventSink for MemorySink {
    fn emit(&self, event: ProviderExecutionEvent) {
        self.events.lock().unwrap().push(event);
    }
}
