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
