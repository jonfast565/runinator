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

use runinator_blob_core::BlobError;

const CRLF: &[u8] = b"\r\n";

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

#[cfg(test)]
#[path = "chunked_tests.rs"]
mod tests;
