//! byte ranges, the random-access half of the store.
//!
//! Parse one `bytes=` range in one of the three forms S3 supports.
//! Reject multi-range requests. Returning only one of two requested ranges could corrupt the
//! caller's result.

use std::fmt;

use crate::errors::BlobError;

/// an unresolved range, as written by the caller. it cannot be turned into offsets without knowing
/// the object's size, which is why resolution is a separate step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ByteRange {
    /// `bytes=start-end`, both inclusive; `end` absent means "to the last byte".
    From { start: u64, end: Option<u64> },
    /// `bytes=-n`, the final `n` bytes.
    Suffix(u64),
}

impl ByteRange {
    /// parse an http `Range` header value. returns `Ok(None)` for an absent header so callers can
    /// thread the optional through without a second match.
    pub fn parse_header(value: Option<&str>) -> Result<Option<Self>, BlobError> {
        let Some(value) = value else {
            return Ok(None);
        };
        let value = value.trim();
        let Some(spec) = value.strip_prefix("bytes=") else {
            return Err(BlobError::BadRequest(format!(
                "unsupported range unit in {value:?}"
            )));
        };
        if spec.contains(',') {
            return Err(BlobError::BadRequest(
                "multiple byte ranges are not supported".into(),
            ));
        }
        let (start, end) = spec
            .split_once('-')
            .ok_or_else(|| BlobError::BadRequest(format!("malformed range {value:?}")))?;
        let (start, end) = (start.trim(), end.trim());
        if start.is_empty() {
            let length = end
                .parse::<u64>()
                .map_err(|_| BlobError::BadRequest(format!("malformed suffix range {value:?}")))?;
            if length == 0 {
                return Err(BlobError::BadRequest("suffix range of zero bytes".into()));
            }
            return Ok(Some(ByteRange::Suffix(length)));
        }
        let start = start
            .parse::<u64>()
            .map_err(|_| BlobError::BadRequest(format!("malformed range start in {value:?}")))?;
        if end.is_empty() {
            return Ok(Some(ByteRange::From { start, end: None }));
        }
        let end = end
            .parse::<u64>()
            .map_err(|_| BlobError::BadRequest(format!("malformed range end in {value:?}")))?;
        if end < start {
            return Err(BlobError::BadRequest(format!(
                "range end precedes start in {value:?}"
            )));
        }
        Ok(Some(ByteRange::From {
            start,
            end: Some(end),
        }))
    }

    /// turn this into concrete offsets against a known object size.
    pub fn resolve(&self, size: u64) -> Result<ResolvedRange, BlobError> {
        let unsatisfiable = || BlobError::RangeNotSatisfiable {
            range: self.to_string(),
            size,
        };
        match *self {
            ByteRange::Suffix(length) => {
                if size == 0 {
                    return Err(unsatisfiable());
                }
                let start = size.saturating_sub(length);
                Ok(ResolvedRange {
                    start,
                    length: size - start,
                    total: size,
                })
            }
            ByteRange::From { start, end } => {
                if start >= size {
                    return Err(unsatisfiable());
                }
                // Clamp an end past the last byte, matching S3.
                let last = end.unwrap_or(size - 1).min(size - 1);
                Ok(ResolvedRange {
                    start,
                    length: last - start + 1,
                    total: size,
                })
            }
        }
    }
}

impl fmt::Display for ByteRange {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ByteRange::Suffix(length) => write!(formatter, "bytes=-{length}"),
            ByteRange::From { start, end: None } => write!(formatter, "bytes={start}-"),
            ByteRange::From {
                start,
                end: Some(end),
            } => write!(formatter, "bytes={start}-{end}"),
        }
    }
}

/// concrete offsets into an object of known size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedRange {
    pub start: u64,
    pub length: u64,
    pub total: u64,
}

impl ResolvedRange {
    /// the inclusive last byte this range covers.
    pub fn last(&self) -> u64 {
        self.start + self.length - 1
    }

    /// the `Content-Range` header value for a 206 response.
    pub fn content_range(&self) -> String {
        format!("bytes {}-{}/{}", self.start, self.last(), self.total)
    }
}

#[cfg(test)]
#[path = "range_tests.rs"]
mod range_tests;
