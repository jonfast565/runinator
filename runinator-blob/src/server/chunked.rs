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
pub struct AwsChunkedDecoder {
    state: DecodeState,
    hasher: Sha256,
    trailer_checksum: Option<String>,
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

impl Decoder for AwsChunkedDecoder {
    type Item = Bytes;
    type Error = std::io::Error;

    fn decode(&mut self, source: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        loop {
            match self.state {
                DecodeState::Header => {
                    let Some(line_end) = find(source, 0, CRLF) else {
                        if source.len() > MAX_LINE_BYTES {
                            return Err(invalid("aws-chunked header is too long"));
                        }
                        return Ok(None);
                    };
                    let line = source.split_to(line_end + CRLF.len());
                    let header = std::str::from_utf8(&line[..line_end])
                        .map_err(|_| invalid("aws-chunked header is not utf-8"))?;
                    let size_text = header.split(';').next().unwrap_or("").trim();
                    let size = usize::from_str_radix(size_text, 16).map_err(|_| {
                        invalid(&format!(
                            "aws-chunked size '{size_text}' is not hexadecimal"
                        ))
                    })?;
                    self.state = if size == 0 {
                        DecodeState::Trailers
                    } else {
                        DecodeState::Data(size)
                    };
                }
                DecodeState::Data(remaining) => {
                    if remaining == 0 {
                        self.state = DecodeState::DataCrlf;
                        continue;
                    }
                    if source.is_empty() {
                        return Ok(None);
                    }
                    let read = remaining.min(source.len());
                    let data = source.split_to(read).freeze();
                    self.hasher.update(&data);
                    self.state = DecodeState::Data(remaining - read);
                    return Ok(Some(data));
                }
                DecodeState::DataCrlf => {
                    if source.len() < CRLF.len() {
                        return Ok(None);
                    }
                    if &source[..CRLF.len()] != CRLF {
                        return Err(invalid("aws-chunked data has no trailing crlf"));
                    }
                    source.advance(CRLF.len());
                    self.state = DecodeState::Header;
                }
                DecodeState::Trailers => {
                    let Some(line_end) = find(source, 0, CRLF) else {
                        if source.len() > MAX_LINE_BYTES {
                            return Err(invalid("aws-chunked trailer is too long"));
                        }
                        return Ok(None);
                    };
                    let line = source.split_to(line_end + CRLF.len());
                    if line_end == 0 {
                        if let Some(expected) = self.trailer_checksum.take() {
                            let actual = hex::encode(self.hasher.clone().finalize());
                            let expected =
                                runinator_blob_core::sha256_from_checksum_header(&expected)
                                    .ok_or_else(|| {
                                        invalid("aws-chunked checksum trailer is malformed")
                                    })?;
                            if !expected.eq_ignore_ascii_case(&actual) {
                                return Err(invalid(&format!(
                                    "aws-chunked checksum mismatch: expected {expected}, computed {actual}"
                                )));
                            }
                        }
                        self.state = DecodeState::Done;
                        continue;
                    }
                    let trailer = std::str::from_utf8(&line[..line_end])
                        .map_err(|_| invalid("aws-chunked trailer is not utf-8"))?;
                    if let Some((name, value)) = trailer.split_once(':') {
                        if name.trim().eq_ignore_ascii_case("x-amz-checksum-sha256") {
                            self.trailer_checksum = Some(value.trim().to_string());
                        }
                    }
                }
                DecodeState::Done => {
                    source.clear();
                    return Ok(None);
                }
            }
        }
    }

    fn decode_eof(&mut self, source: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if let Some(item) = self.decode(source)? {
            return Ok(Some(item));
        }
        if matches!(self.state, DecodeState::Done) {
            return Ok(None);
        }
        Err(invalid("aws-chunked body ended before its final trailer"))
    }
}

fn invalid(message: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message.to_string())
}

/// fail after `limit` decoded bytes instead of silently truncating the object.
pub struct LimitedReader<R> {
    inner: R,
    limit: u64,
    consumed: u64,
}

impl<R> LimitedReader<R> {
    pub fn new(inner: R, limit: u64) -> Self {
        Self {
            inner,
            limit,
            consumed: 0,
        }
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for LimitedReader<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        target: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        if target.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        let remaining = self.limit.saturating_sub(self.consumed);
        let allowed = usize::try_from(remaining.saturating_add(1))
            .unwrap_or(usize::MAX)
            .min(target.remaining());
        let unfilled = target.initialize_unfilled_to(allowed);
        let mut limited = ReadBuf::new(unfilled);
        match Pin::new(&mut self.inner).poll_read(cx, &mut limited) {
            Poll::Ready(Ok(())) => {
                let read = limited.filled().len();
                let consumed = self.consumed.saturating_add(read as u64);
                if consumed > self.limit {
                    return Poll::Ready(Err(invalid(
                        "decoded upload exceeds the configured limit",
                    )));
                }
                target.advance(read);
                self.consumed = consumed;
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
            Poll::Pending => Poll::Pending,
        }
    }
}

#[cfg(test)]
#[path = "chunked_tests.rs"]
mod tests;
