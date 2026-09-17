#[allow(unused_imports)]
use super::*;

pub(super) struct Renewal {
    pub(super) task: tokio::task::JoinHandle<()>,
    pub(super) valid: Arc<AtomicBool>,
}

impl Drop for Renewal {
    fn drop(&mut self) {
        self.task.abort();
    }
}
