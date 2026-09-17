#[allow(unused_imports)]
use super::*;

/// an org plus the caller's role in it, returned from `/orgs/me`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgMembershipView {
    pub org: Organization,
    pub role: OrgRole,
}
