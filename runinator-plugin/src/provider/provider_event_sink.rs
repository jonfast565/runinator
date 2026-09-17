#[allow(unused_imports)]
use super::*;

pub trait ProviderEventSink: Send + Sync {
    fn emit(&self, event: ProviderExecutionEvent);

    /// Transfer this effect's terminal-control receiver to a provider that owns an interactive
    /// session. The default preserves compatibility for sinks outside the worker runtime.
    fn take_terminal_control(&self) -> Option<Receiver<ProviderTerminalControl>> {
        None
    }
}
