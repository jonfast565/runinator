//! multipart upload types.
//!
//! multipart exists because the aws sdks switch to it above a size threshold on their own — a client
//! uploading a 20 MB function artifact never asks whether the server supports it. the local backend
//! stages each part as its own file and concatenates on completion, which is why parts are numbered
//! and ordered rather than streamed.

use serde::{Deserialize, Serialize};

/// s3's part-number bounds.
pub const MIN_PART_NUMBER: u32 = 1;
pub const MAX_PART_NUMBER: u32 = 10_000;

/// a part the client claims to have uploaded, as sent in a completion request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletedPart {
    pub part_number: u32,
    /// the etag the upload-part response returned, echoed back for verification.
    pub etag: String,
}

/// an in-progress upload's identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MultipartUpload {
    pub upload_id: String,
    pub bucket: String,
    pub key: String,
}
