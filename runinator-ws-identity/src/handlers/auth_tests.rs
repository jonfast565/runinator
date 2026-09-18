//! agent enrollment authorization.
use super::*;
use std::collections::BTreeMap;

fn labels(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
        .collect()
}

// a token minted without `--label` used to reject every desktop agent, because the agent always
// advertises identity labels it will not let anyone override.
#[test]
fn a_token_granting_nothing_still_admits_the_forced_desktop_identity() {
    let presented = labels(&[("pool", "desktop"), ("runner", "desktop")]);
    assert_eq!(ungranted_label(&presented, &BTreeMap::new()), None);
}

#[test]
fn an_operator_chosen_label_still_needs_an_explicit_grant() {
    let presented = labels(&[("pool", "desktop"), ("zone", "onprem")]);
    let refused = ungranted_label(&presented, &BTreeMap::new()).expect("zone was never granted");
    assert_eq!(refused.0.as_str(), "zone");
}

#[test]
fn a_granted_label_is_admitted() {
    let presented = labels(&[("zone", "onprem")]);
    let granted = labels(&[("zone", "onprem")]);
    assert_eq!(ungranted_label(&presented, &granted), None);
}

#[test]
fn a_granted_key_does_not_admit_a_different_value() {
    let presented = labels(&[("zone", "eu")]);
    let granted = labels(&[("zone", "onprem")]);
    let refused = ungranted_label(&presented, &granted).expect("eu is not the granted zone");
    assert_eq!(refused.0.as_str(), "zone");
}

// the identity exemption is the exact pair, not the key: otherwise a token holder could claim the
// production pool and receive work routed to it.
#[test]
fn the_identity_exemption_does_not_extend_to_another_pool() {
    let presented = labels(&[("pool", "production")]);
    let refused =
        ungranted_label(&presented, &BTreeMap::new()).expect("only pool=desktop is forced");
    assert_eq!(refused.0.as_str(), "pool");
    assert_eq!(refused.1.as_str(), "production");
}
