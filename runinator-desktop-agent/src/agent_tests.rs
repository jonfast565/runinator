//! start-latch recovery: a failed or settled lifecycle must not block a later Start.

use super::*;

#[test]
fn start_is_allowed_when_idle() {
    let shared = Shared::default();
    assert!(!should_skip_start(&shared));
}

#[test]
fn start_is_skipped_while_busy() {
    let mut shared = Shared::default();
    shared.busy = true;
    assert!(should_skip_start(&shared));
}

#[test]
fn start_is_allowed_after_a_failed_start() {
    // the GUI shows Start when `running` is false (failed registration, settled stop). a leftover
    // handle must not make that click a no-op — there is no live handle in this fixture, which is
    // the same skip outcome as a finished one (`is_finished` => not live).
    let mut shared = Shared::default();
    shared.status.running = false;
    shared.connection = ConnectionState::Stopped;
    assert!(!should_skip_start(&shared));
}

#[test]
fn start_is_allowed_when_running_flag_is_set_without_a_live_handle() {
    // inconsistent leftover from a lifecycle that settled without being reaped.
    let mut shared = Shared::default();
    shared.status.running = true;
    assert!(!should_skip_start(&shared));
}
