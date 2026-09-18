//! Bounded scans and shared origin normalization.
use super::*;

#[test]
fn incomplete_scan_cannot_advance_checkpoint() {
    assert!(require_complete_page(true).is_err());
    assert!(require_complete_page(false).is_ok());
}

#[test]
fn polling_preserves_self_origin_operation_marker() {
    let payload = json!({"body":"<!-- runinator-operation:operation-42 -->"});
    assert_eq!(
        operation_provenance(&payload),
        json!({"operation_key":"operation-42"})
    );
}

// every stream drops items older than its mark itself, because `github_collect` returns whole
// pages rather than filtering. the review stream once skipped this and spent a request per pull
// request in the repository on every poll, which outgrew its own poll timeout.
#[test]
fn an_item_older_than_the_mark_is_skipped() {
    assert!(github_predates_mark(
        "2026-09-18T21:00:00Z",
        Some("2026-09-18T21:17:58Z")
    ));
}

#[test]
fn an_item_at_or_after_the_mark_is_kept() {
    assert!(!github_predates_mark(
        "2026-09-18T21:17:58Z",
        Some("2026-09-18T21:17:58Z")
    ));
    assert!(!github_predates_mark(
        "2026-09-18T22:00:00Z",
        Some("2026-09-18T21:17:58Z")
    ));
}

// a first poll has no mark, so nothing may be skipped as stale.
#[test]
fn nothing_is_stale_without_a_mark() {
    assert!(!github_predates_mark("2026-09-18T21:00:00Z", None));
}

// an item whose timestamp GitHub omitted must not be silently dropped as ancient.
#[test]
fn an_undated_item_is_not_treated_as_stale() {
    assert!(!github_predates_mark("", Some("2026-09-18T21:17:58Z")));
}
