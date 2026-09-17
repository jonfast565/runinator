#[allow(unused_imports)]
use super::*;

#[derive(Default)]
pub(super) struct RecordingSink(pub(super) Mutex<Vec<ProviderExecutionEvent>>);

impl ProviderEventSink for RecordingSink {
    fn emit(&self, event: ProviderExecutionEvent) {
        self.0.lock().unwrap().push(event);
    }
}
