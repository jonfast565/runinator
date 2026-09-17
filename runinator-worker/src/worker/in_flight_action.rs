#[allow(unused_imports)]
use super::*;

/// One provider effect tracked so a targeted or run-wide control cancellation can stop it.
#[derive(Clone)]
pub(crate) struct InFlightAction {
    pub(crate) workflow_run_id: Uuid,
    pub(crate) token: CancellationToken,
    pub(crate) canceled_by_control: Arc<AtomicBool>,
    pub(crate) terminal: Sender<ProviderTerminalControl>,
}
