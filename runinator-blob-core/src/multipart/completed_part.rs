#[allow(unused_imports)]
use super::*;

/// a part the client claims to have uploaded, as sent in a completion request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletedPart {
    pub part_number: u32,
    /// the etag the upload-part response returned, echoed back for verification.
    pub etag: String,
}
