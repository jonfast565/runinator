//! the `aws-chunked` content encoding.
//!
//! AWS SDKs and the AWS CLI may frame uploads as `aws-chunked` so they can add a checksum trailer.
//! Remove that framing before the bytes reach the store. Otherwise length prefixes would be stored
//! as part of every object.
//!
//! ```text
//! <hex-length>[;chunk-signature=...]\r\n<data>\r\n
//! ...
//! 0[;chunk-signature=...]\r\n
//! <trailer-name>:<value>\r\n
//! \r\n
//! ```

use bytes::{Buf, Bytes, BytesMut};
use sha2::{Digest, Sha256};
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, ReadBuf};
use tokio_util::codec::{Decoder, FramedRead};

use runinator_blob_core::BlobError;

const CRLF: &[u8] = b"\r\n";
const MAX_LINE_BYTES: usize = 16 * 1024;

/// strip `aws-chunked` framing, returning the payload bytes.
pub fn decode(body: &[u8]) -> Result<Vec<u8>, BlobError> {
    let mut out = Vec::with_capacity(body.len());
    let mut cursor = 0usize;
    loop {
        let line_end = find(body, cursor, CRLF).ok_or_else(|| {
            BlobError::BadRequest("aws-chunked body ended inside a chunk header".into())
        })?;
        let header = std::str::from_utf8(&body[cursor..line_end])
            .map_err(|_| BlobError::BadRequest("aws-chunked header is not utf-8".into()))?;
        // the extension after `;` carries the per-chunk signature in signed mode; the size is all
        // that matters for reassembly.
        let size_text = header.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_text, 16).map_err(|_| {
            BlobError::BadRequest(format!("aws-chunked size '{size_text}' is not hexadecimal"))
        })?;
        cursor = line_end + CRLF.len();
        if size == 0 {
            // whatever follows is the trailer, which the store does not persist.
            return Ok(out);
        }
        let end = cursor
            .checked_add(size)
            .filter(|end| *end <= body.len())
            .ok_or_else(|| {
                BlobError::BadRequest(
                    "aws-chunked chunk claims more bytes than the body holds".into(),
                )
            })?;
        out.extend_from_slice(&body[cursor..end]);
        cursor = end;
        if body[cursor..].starts_with(CRLF) {
            cursor += CRLF.len();
        }
    }
}

fn find(haystack: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    if from >= haystack.len() {
        return None;
    }
    haystack[from..]
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|at| at + from)
}

/// decode `aws-chunked` framing incrementally and verify a checksum trailer when present.
pub fn reader<R>(source: R) -> tokio_util::io::StreamReader<FramedRead<R, AwsChunkedDecoder>, Bytes>
where
    R: tokio::io::AsyncRead + Unpin,
{
    tokio_util::io::StreamReader::new(FramedRead::new(source, AwsChunkedDecoder::default()))
}

#[derive(Default)]
enum DecodeState {
    #[default]
    Header,
    Data(usize),
    DataCrlf,
    Trailers,
    Done,
}

fn invalid(message: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message.to_string())
}

/// fail after `limit` decoded bytes instead of silently truncating the object.

#[cfg(test)]
#[path = "chunked_tests.rs"]
mod tests;

mod aws_chunked_decoder;
pub use aws_chunked_decoder::AwsChunkedDecoder;

mod limited_reader;
pub use limited_reader::LimitedReader;
