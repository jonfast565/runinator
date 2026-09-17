//! multipart upload types.
//!
//! multipart exists because the aws sdks switch to it above a size threshold on their own — a client
//! uploading a 20 MB function artifact never asks whether the server supports it. the local backend
//! stages each part as its own file and concatenates on completion, which is why parts are numbered
//! and ordered rather than streamed.

use serde::{Deserialize, Serialize};

/// S3's part-number bounds.
pub const MIN_PART_NUMBER: u32 = 1;
pub const MAX_PART_NUMBER: u32 = 10_000;

mod completed_part;
pub use completed_part::CompletedPart;

mod multipart_upload;
pub use multipart_upload::MultipartUpload;
