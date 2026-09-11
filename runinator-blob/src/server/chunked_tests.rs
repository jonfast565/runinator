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

#[tokio::test]
async fn streams_and_verifies_a_sha256_trailer() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let checksum =
        runinator_blob_core::sha256_hex_to_base64(&runinator_blob_core::sha256_hex(b"hello world"))
            .unwrap();
    let framed =
        format!("5\r\nhello\r\n6\r\n world\r\n0\r\nx-amz-checksum-sha256:{checksum}\r\n\r\n");
    let (mut writer, source) = tokio::io::duplex(8);
    let framed = framed.into_bytes();
    tokio::spawn(async move {
        for byte in framed {
            writer.write_all(&[byte]).await.unwrap();
        }
    });
    let mut decoded = reader(source);
    let mut body = Vec::new();
    decoded.read_to_end(&mut body).await.unwrap();
    assert_eq!(body, b"hello world");
}

#[tokio::test]
async fn rejects_a_bad_streaming_checksum_and_an_oversized_body() {
    use tokio::io::AsyncReadExt;

    let framed = b"5\r\nhello\r\n0\r\nx-amz-checksum-sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=\r\n\r\n";
    let mut decoded = reader(std::io::Cursor::new(framed.to_vec()));
    assert!(decoded.read_to_end(&mut Vec::new()).await.is_err());

    let mut limited = LimitedReader::new(std::io::Cursor::new(b"hello".to_vec()), 4);
    assert!(limited.read_to_end(&mut Vec::new()).await.is_err());
}
