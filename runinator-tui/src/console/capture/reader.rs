#[allow(unused_imports)]
use super::*;

pub(crate) struct Reader {
    pub(super) handle: JoinHandle<()>,
    pub(super) done: Receiver<()>,
}

impl Reader {
    pub(crate) fn finish(self) {
        if self.done.recv_timeout(Duration::from_millis(250)).is_ok() {
            let _ = self.handle.join();
        }
        // An inherited child writer may outlive the prompt. Detaching the drain prevents exit from
        // hanging and keeps the read end open until that writer closes naturally.
    }
}
