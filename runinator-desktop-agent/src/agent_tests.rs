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

// the phase a Start click opens: `busy` alone cannot say whether a start or a stop is in flight, and
// only a start is cancellable.
#[test]
fn a_start_in_flight_offers_cancel() {
    let mut shared = Shared::default();
    shared.busy = true;
    shared.starting = true;
    assert_eq!(control_state(&shared), Control::Starting);
    // and a second Start click during it stays a no-op.
    assert!(should_skip_start(&shared));
}

#[test]
fn a_stop_in_flight_offers_nothing() {
    let mut shared = Shared::default();
    shared.busy = true;
    assert_eq!(control_state(&shared), Control::Stopping);
}

#[test]
fn an_idle_agent_offers_start() {
    assert_eq!(control_state(&Shared::default()), Control::Startable);
}

// a lifecycle that settled (no live handle) is startable again whatever the `running` flag says,
// which is the same leftover case `start_is_allowed_when_running_flag_is_set_without_a_live_handle`
// covers for the start latch.
#[test]
fn a_settled_lifecycle_is_startable_not_running() {
    let mut shared = Shared::default();
    shared.status.running = true;
    assert_eq!(control_state(&shared), Control::Startable);
}
