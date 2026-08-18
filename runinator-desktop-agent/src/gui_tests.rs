//! covers the status indicator: which connection state paints which dot, and the config gate.

use super::*;

// the whole point of the amber/red split — an operator glancing at the dot has to be able to tell
// "still trying" from "stopped and waiting for you".
#[test]
fn a_reconnect_attempt_is_amber_and_a_give_up_is_red() {
    let reconnecting = present_status(
        &ConnectionState::Reconnecting {
            retry_secs: 8,
            attempt: 3,
            max_attempts: Some(10),
        },
        false,
    );
    assert_eq!(reconnecting.color, DOT_AMBER);
    assert!(matches!(reconnecting.tray_color, TrayColor::Reconnecting));

    let disconnected = present_status(
        &ConnectionState::Disconnected {
            attempts: 10,
            reason: "connection closed".to_string(),
        },
        false,
    );
    assert_eq!(disconnected.color, DOT_RED);
    assert!(matches!(disconnected.tray_color, TrayColor::Disconnected));
}

#[test]
fn a_reconnect_attempt_shows_its_progress_against_the_budget() {
    let presentation = present_status(
        &ConnectionState::Reconnecting {
            retry_secs: 8,
            attempt: 3,
            max_attempts: Some(10),
        },
        false,
    );
    assert!(
        presentation.label.contains("3/10"),
        "{}",
        presentation.label
    );
    assert!(
        presentation.tooltip.contains("retry in 8s"),
        "{}",
        presentation.tooltip
    );
}

// an unlimited budget has no denominator, so a bare count would be meaningless noise.
#[test]
fn an_unlimited_budget_shows_no_attempt_count() {
    let presentation = present_status(
        &ConnectionState::Reconnecting {
            retry_secs: 8,
            attempt: 42,
            max_attempts: None,
        },
        false,
    );
    assert_eq!(presentation.label, "● reconnecting");
    assert!(
        !presentation.tooltip.contains("42"),
        "{}",
        presentation.tooltip
    );
}

// the tray is refreshed only when the tooltip changes, so two states that read the same would
// freeze the icon on whichever landed first.
#[test]
fn the_tooltip_distinguishes_a_stop_from_a_give_up() {
    let stopped = present_status(&ConnectionState::Stopped, false);
    let disconnected = present_status(
        &ConnectionState::Disconnected {
            attempts: 10,
            reason: "connection closed".to_string(),
        },
        false,
    );
    assert_ne!(stopped.tooltip, disconnected.tooltip);
    assert!(
        disconnected.tooltip.contains("10 attempts"),
        "{}",
        disconnected.tooltip
    );
}

#[test]
fn a_transition_in_flight_outranks_the_underlying_phase() {
    let presentation = present_status(
        &ConnectionState::Disconnected {
            attempts: 10,
            reason: "connection closed".to_string(),
        },
        true,
    );
    assert_eq!(presentation.label, "● working…");
}
