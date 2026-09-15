//! Full-screen server-selection precedence.

use super::*;

#[test]
fn clap_default_does_not_count_as_an_explicit_server() {
    assert!(!api_base_was_explicit_in(
        None,
        ["runinatorctl".into(), "tui".into()]
    ));
}

#[test]
fn flag_and_environment_each_bypass_selection() {
    assert!(api_base_was_explicit_in(
        None,
        [
            "runinatorctl".into(),
            "--api-base-url=https://example.test".into(),
            "tui".into()
        ],
    ));
    assert!(api_base_was_explicit_in(
        Some("https://example.test".into()),
        ["runinatorctl".into(), "tui".into()],
    ));
}

#[test]
fn empty_environment_value_still_requires_selection() {
    assert!(!api_base_was_explicit_in(
        Some("".into()),
        ["runinatorctl".into(), "tui".into()],
    ));
}
