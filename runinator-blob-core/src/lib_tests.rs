//! covers the `blob://` URI form shared by every artifact row.

use super::*;

#[test]
fn round_trips_a_blob_uri() {
    let key = ObjectKey::parse("sha256/abc.zip").unwrap();
    let uri = blob_uri(FUNCTION_ARTIFACT_BUCKET, &key);
    assert_eq!(uri, "blob://runinator-function-artifacts/sha256/abc.zip");
    let (bucket, parsed) = parse_blob_uri(&uri).unwrap();
    assert_eq!(bucket, FUNCTION_ARTIFACT_BUCKET);
    assert_eq!(parsed, key);
}

#[test]
fn rejects_non_blob_uris() {
    // Other URI schemes and plain paths are not object references.
    assert!(parse_blob_uri("/var/lib/runinator/artifacts/run/report.txt").is_none());
    assert!(parse_blob_uri("https://example.com/a").is_none());
    assert!(parse_blob_uri("blob://bucket").is_none());
    assert!(parse_blob_uri("blob://UPPER/key").is_none());
    assert!(parse_blob_uri("blob://bucket-name/../escape").is_none());
}
