#[allow(unused_imports)]
use super::*;

/// a verified local credential lookup: the user plus the stored argon2 hash to check against.
#[derive(Debug, Clone)]
pub struct LocalCredential {
    pub user: User,
    pub password_hash: String,
}
