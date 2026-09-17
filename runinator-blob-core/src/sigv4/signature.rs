#[allow(unused_imports)]
use super::*;

/// a computed signature plus the pieces a caller needs to put it on the wire.
#[derive(Debug, Clone)]
pub struct Signature {
    pub signature: String,
    pub signed_headers: String,
    pub credential_scope: String,
    pub amz_date: String,
}

impl Signature {
    /// the `Authorization` header value for a header-signed request.
    pub fn authorization_header(&self, access_key_id: &str) -> String {
        format!(
            "{ALGORITHM} Credential={access_key_id}/{}, SignedHeaders={}, Signature={}",
            self.credential_scope, self.signed_headers, self.signature
        )
    }
}
