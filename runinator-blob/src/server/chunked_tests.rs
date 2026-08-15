//! covers aws-chunked stripping, including the signed-chunk extension and the trailer.

use super::*;

#[test]
fn strips_framing_and_trailer() {
    let body = b"5\r\nhello\r\n6\r\n world\r\n0\r\nx-amz-checksum-crc32:AAAAAA==\r\n\r\n";
    assert_eq!(decode(body).unwrap(), b"hello world");
}

#[test]
fn ignores_the_chunk_signature_extension() {
    let body = b"5;chunk-signature=deadbeef\r\nhello\r\n0;chunk-signature=cafe\r\n\r\n";
    assert_eq!(decode(body).unwrap(), b"hello");
}

#[test]
fn handles_an_empty_payload() {
    assert_eq!(decode(b"0\r\n\r\n").unwrap(), Vec::<u8>::new());
}

#[test]
fn rejects_a_truncated_or_malformed_body() {
    // a chunk that claims more bytes than are present must not read past the buffer.
    assert!(decode(b"ff\r\nshort").is_err());
    assert!(decode(b"zz\r\ndata\r\n0\r\n\r\n").is_err());
    assert!(decode(b"5").is_err());
}
