#[allow(unused_imports)]
use super::*;

pub(super) struct TerminalClaim;

impl Drop for TerminalClaim {
    fn drop(&mut self) {
        TERMINAL_ACTIVE.store(false, Ordering::Release);
    }
}
