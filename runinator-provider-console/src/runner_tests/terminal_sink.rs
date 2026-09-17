#[allow(unused_imports)]
use super::*;

pub(super) struct TerminalSink {
    pub(super) events: Mutex<Vec<ProviderExecutionEvent>>,
    pub(super) controls: Mutex<Option<Receiver<ProviderTerminalControl>>>,
}

impl ProviderEventSink for TerminalSink {
    fn emit(&self, event: ProviderExecutionEvent) {
        self.events.lock().unwrap().push(event);
    }

    fn take_terminal_control(&self) -> Option<Receiver<ProviderTerminalControl>> {
        self.controls.lock().unwrap().take()
    }
}
