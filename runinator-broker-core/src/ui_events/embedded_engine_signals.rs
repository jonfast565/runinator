#[allow(unused_imports)]
use super::*;

/// Process-local signals shared only by the composition root that embeds an engine.
///
/// They reduce the time until durable work is polled after an HTTP write. Standalone engines receive
/// no handle and remain correct through their normal polling intervals.
#[derive(Clone, Default)]
pub struct EmbeddedEngineSignals {
    pub(super) workflow_vm: Arc<Notify>,
    pub(super) agent_directives: Arc<Notify>,
}

impl EmbeddedEngineSignals {
    pub fn new() -> Self {
        Self::default()
    }

    /// Prompt the VM driver to check its durable continuation queue now.
    pub fn nudge_workflow_vm(&self) {
        self.workflow_vm.notify_one();
    }

    /// Prompt the agent-directive publisher to drain its durable outbox now.
    pub fn nudge_agent_directives(&self) {
        self.agent_directives.notify_one();
    }

    /// A signal the engine loop can await. Kept separate from the public nudge method so callers
    /// can only reduce latency, never take responsibility for consuming durable work.
    pub fn workflow_vm_notifier(&self) -> Arc<Notify> {
        self.workflow_vm.clone()
    }

    /// A signal the engine loop can await for its durable agent-directive outbox.
    pub fn agent_directives_notifier(&self) -> Arc<Notify> {
        self.agent_directives.clone()
    }
}
