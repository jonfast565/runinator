#[allow(unused_imports)]
use super::*;

pub struct PresentedSignature {
    pub access_key_id: String,
    pub credential_scope: String,
    pub signed_headers: String,
    pub signature: String,
    pub amz_date: String,
    /// Present only for a presigned URL. Limits how long the signature stays valid.
    pub expires_in: Option<i64>,
}
