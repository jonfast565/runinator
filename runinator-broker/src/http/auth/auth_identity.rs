#[allow(unused_imports)]
use super::*;

/// the authenticated identity attached to a request: `None` when broker auth is disabled (every
/// request is anonymous) — handlers treat that as "no authz constraints".
#[derive(Clone)]
pub struct AuthIdentity(pub Option<ReplicaClaims>);
