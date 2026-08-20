//! covers the scheduling-policy vocabulary shared by the trigger loop, the rexrap header, and the ui
//! catalog.

use super::*;
use crate::json;

/// `ALL` is what the ui catalog offers. a variant missing from it is a policy an operator cannot
/// pick even though the trigger loop honors it, which is silent — so pin the list and its wire
/// names together.
#[test]
fn every_concurrency_policy_is_listed_exactly_once_with_its_wire_name() {
    let names: Vec<&str> = ConcurrencyPolicy::ALL.iter().map(|p| p.as_str()).collect();
    assert_eq!(names, ["allow", "skip", "queue", "cancel_previous"]);
    for policy in ConcurrencyPolicy::ALL {
        assert_eq!(
            ConcurrencyPolicy::from_str_opt(policy.as_str()),
            Some(policy)
        );
        assert_eq!(
            serde_json::to_value(policy).unwrap(),
            serde_json::Value::String(policy.as_str().into()),
            "the serde name is the author-facing name the rexrap header spells"
        );
    }
    assert_eq!(ConcurrencyPolicy::from_str_opt("cancel"), None);
}

/// the header is read back off a stored definition, so an absent or malformed entry has to degrade
/// to the pre-policy behavior rather than declining a firing.
#[test]
fn concurrency_metadata_falls_back_to_unlimited() {
    assert_eq!(
        WorkflowConcurrency::from_metadata(&json!({})),
        WorkflowConcurrency::unlimited()
    );
    assert_eq!(
        WorkflowConcurrency::from_metadata(&json!({ "concurrency": "two" })),
        WorkflowConcurrency::unlimited()
    );
    assert_eq!(
        WorkflowConcurrency::from_metadata(
            &json!({ "concurrency": { "max_concurrent_runs": 2, "on_conflict": "queue" } })
        ),
        WorkflowConcurrency {
            max_concurrent_runs: 2,
            on_conflict: ConcurrencyPolicy::Queue,
        }
    );
}

/// an unlimited cap and an `allow` policy both mean "never decline", which is what lets the trigger
/// loop skip counting active runs.
#[test]
fn only_a_capped_non_allow_policy_is_enforced() {
    assert!(!WorkflowConcurrency::unlimited().is_enforced());
    assert!(
        !WorkflowConcurrency {
            max_concurrent_runs: 4,
            on_conflict: ConcurrencyPolicy::Allow,
        }
        .is_enforced()
    );
    assert!(
        WorkflowConcurrency {
            max_concurrent_runs: 4,
            on_conflict: ConcurrencyPolicy::Skip,
        }
        .is_enforced()
    );
}
