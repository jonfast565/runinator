#[allow(unused_imports)]
use super::*;

pub(super) struct Reader {
    pub(super) handle: JoinHandle<()>,
    pub(super) done: Receiver<()>,
}

impl Reader {
    pub(super) fn finish(self) {
        if self.done.recv_timeout(Duration::from_millis(250)).is_ok() {
            let _ = self.handle.join();
        }
        // A child may have inherited stdout/stderr and still own a writer. Dropping an unfinished
        // JoinHandle detaches the draining reader so shutdown cannot hang or close the pipe under
        // that child, which otherwise surfaces as a broken-pipe panic on WSL.
    }
}
