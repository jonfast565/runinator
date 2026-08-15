//! covers the xml documents, especially escaping of attacker-influenced keys.

use super::*;
use runinator_blob_core::listing::ObjectSummary;

fn at(seconds: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(seconds, 0).unwrap()
}

#[test]
fn escapes_element_text() {
    assert_eq!(
        escape("a&b<c>\"d\"'e'"),
        "a&amp;b&lt;c&gt;&quot;d&quot;&apos;e&apos;"
    );
}

#[test]
fn renders_a_listing_with_a_hostile_key() {
    let response = ListResponse {
        objects: vec![ObjectSummary {
            key: "a&<b>.txt".into(),
            size: 3,
            sha256: "abc".into(),
            last_modified: at(0),
        }],
        common_prefixes: vec!["dir/".into()],
        is_truncated: true,
        next_continuation_token: Some("dir/".into()),
    };
    let xml = list_objects_v2("bucket", Some("a"), Some("/"), 10, &response);
    assert!(xml.contains("<Key>a&amp;&lt;b&gt;.txt</Key>"));
    assert!(!xml.contains("<b>"));
    assert!(xml.contains("<KeyCount>2</KeyCount>"));
    assert!(xml.contains("<IsTruncated>true</IsTruncated>"));
    assert!(xml.contains("<NextContinuationToken>dir/</NextContinuationToken>"));
    assert!(xml.contains("<CommonPrefixes><Prefix>dir/</Prefix></CommonPrefixes>"));
}

#[test]
fn parses_a_completion_request() {
    let body = r#"<?xml version="1.0" encoding="UTF-8"?>
<CompleteMultipartUpload>
  <Part><PartNumber>1</PartNumber><ETag>&quot;aaa&quot;</ETag></Part>
  <Part><PartNumber>2</PartNumber><ETag>"bbb"</ETag></Part>
</CompleteMultipartUpload>"#;
    let parts = parse_completed_parts(body).unwrap();
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0].part_number, 1);
    assert_eq!(parts[0].etag, "\"aaa\"");
    assert_eq!(parts[1].etag, "\"bbb\"");
}

#[test]
fn rejects_an_empty_or_malformed_completion() {
    assert!(parse_completed_parts("<CompleteMultipartUpload/>").is_err());
    assert!(parse_completed_parts("<Part><ETag>\"a\"</ETag></Part>").is_err());
    assert!(
        parse_completed_parts("<Part><PartNumber>x</PartNumber><ETag>\"a\"</ETag></Part>").is_err()
    );
}

#[test]
fn renders_an_error_document() {
    let xml = error("NoSuchKey", "object not found: a/b", "/bucket/a/b", "req-1");
    assert!(xml.contains("<Code>NoSuchKey</Code>"));
    assert!(xml.contains("<Resource>/bucket/a/b</Resource>"));
}
