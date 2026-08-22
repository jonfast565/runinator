//! covers range header parsing and resolution against a known object size.

use super::*;

#[test]
fn parses_the_three_forms() {
    assert_eq!(
        ByteRange::parse_header(Some("bytes=0-15")).unwrap(),
        Some(ByteRange::From {
            start: 0,
            end: Some(15)
        })
    );
    assert_eq!(
        ByteRange::parse_header(Some("bytes=100-")).unwrap(),
        Some(ByteRange::From {
            start: 100,
            end: None
        })
    );
    assert_eq!(
        ByteRange::parse_header(Some("bytes=-32")).unwrap(),
        Some(ByteRange::Suffix(32))
    );
    assert_eq!(ByteRange::parse_header(None).unwrap(), None);
}

#[test]
fn refuses_multi_range_and_junk() {
    for value in [
        "bytes=0-1,4-5",
        "items=0-1",
        "bytes=",
        "bytes=5-1",
        "bytes=x-y",
    ] {
        assert!(
            ByteRange::parse_header(Some(value)).is_err(),
            "expected {value:?} to be rejected"
        );
    }
}

#[test]
fn resolves_against_size() {
    let resolved = ByteRange::From {
        start: 0,
        end: Some(15),
    }
    .resolve(100)
    .unwrap();
    assert_eq!((resolved.start, resolved.length), (0, 16));
    assert_eq!(resolved.content_range(), "bytes 0-15/100");

    // An end past the last byte is clamped instead of rejected, matching S3.
    let clamped = ByteRange::From {
        start: 90,
        end: Some(999),
    }
    .resolve(100)
    .unwrap();
    assert_eq!((clamped.start, clamped.length), (90, 10));

    // a suffix longer than the object yields the whole object.
    let suffix = ByteRange::Suffix(500).resolve(100).unwrap();
    assert_eq!((suffix.start, suffix.length), (0, 100));
}

#[test]
fn rejects_starts_past_the_end() {
    assert!(matches!(
        ByteRange::From {
            start: 100,
            end: None
        }
        .resolve(100),
        Err(BlobError::RangeNotSatisfiable { .. })
    ));
    assert!(ByteRange::Suffix(4).resolve(0).is_err());
}
