//! covers the response headers, whose exact shape is what aws sdks validate transfers against.

use super::*;
use chrono::{TimeZone, Utc};
use runinator_blob_core::ObjectMeta;

fn meta() -> ObjectMeta {
    ObjectMeta {
        key: "a/b.bin".into(),
        size: 256,
        sha256: "704d3e2f87bd93d41deb050afbab3d33452fc3b6545b5753751c4eb8b129c081".into(),
        content_type: "application/octet-stream".into(),
        last_modified: Utc.with_ymd_and_hms(2026, 8, 15, 0, 0, 0).unwrap(),
        metadata: [("owner".to_string(), "runinator".to_string())]
            .into_iter()
            .collect(),
    }
}

#[test]
fn whole_object_headers_carry_a_base64_checksum() {
    let headers = object_headers(&meta(), None);
    assert_eq!(headers["content-length"], "256");
    assert_eq!(
        headers["etag"],
        "\"704d3e2f87bd93d41deb050afbab3d33452fc3b6545b5753751c4eb8b129c081\""
    );
    // base64 of the same digest; an sdk decodes this and compares against what it received.
    assert_eq!(
        headers["x-amz-checksum-sha256"],
        "cE0+L4e9k9Qd6wUK+6s9M0Uvw7ZUW1dTdRxOuLEpwIE="
    );
    assert_eq!(headers["x-amz-meta-owner"], "runinator");
}

#[test]
fn ranged_headers_omit_the_whole_object_checksum() {
    let range = runinator_blob_core::ResolvedRange {
        start: 0,
        length: 16,
        total: 256,
    };
    let headers = object_headers(&meta(), Some(range));
    assert_eq!(headers["content-length"], "16");
    assert_eq!(headers["content-range"], "bytes 0-15/256");
    // sending the whole-object digest with a partial body makes every sdk ranged download fail its
    // integrity check, which is how large downloads are fetched.
    assert!(!headers.contains_key("x-amz-checksum-sha256"));
}

#[test]
fn a_metadata_value_that_cannot_be_a_header_is_dropped_not_fatal() {
    let mut meta = meta();
    meta.metadata
        .insert("bad".into(), "line one\nline two".into());
    let headers = object_headers(&meta, None);
    assert!(!headers.contains_key("x-amz-meta-bad"));
    assert!(headers.contains_key("x-amz-meta-owner"));
}
