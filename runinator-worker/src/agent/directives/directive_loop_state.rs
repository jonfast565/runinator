#[allow(unused_imports)]
use super::*;

pub(crate) struct DirectiveLoopState {
    pub drained: Arc<std::sync::atomic::AtomicBool>,
    pub restart_requested: Arc<std::sync::atomic::AtomicBool>,
    pub state_changed: Arc<Notify>,
}
