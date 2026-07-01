// organization (tenant) domain/wire types. an org owns workflows/runs/resources; users belong to
// many orgs, each with a role, and act within one active org at a time. authorization within an org
// uses the `OrgRole` ladder; the global `is_admin` on a user is the platform admin that transcends orgs.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// the per-org role ladder. higher variants subsume lower ones (owner ⊇ admin ⊇ member).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrgRole {
    Member,
    Admin,
    Owner,
}

impl OrgRole {
    pub fn as_str(self) -> &'static str {
        match self {
            OrgRole::Member => "member",
            OrgRole::Admin => "admin",
            OrgRole::Owner => "owner",
        }
    }

    pub fn from_str_lossy(raw: &str) -> Option<Self> {
        match raw {
            "member" => Some(OrgRole::Member),
            "admin" => Some(OrgRole::Admin),
            "owner" => Some(OrgRole::Owner),
            _ => None,
        }
    }

    /// true when this role is at least as strong as `required`.
    pub fn allows(self, required: OrgRole) -> bool {
        self >= required
    }
}

/// a tenant. `slug` is the stable, url/label-safe identifier used for routing labels (`org=<slug>`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Organization {
    pub id: Option<Uuid>,
    pub name: String,
    pub slug: String,
    pub disabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// a user's membership in one org, with their role there.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgMembership {
    pub org_id: Uuid,
    pub user_id: Uuid,
    pub role: OrgRole,
    pub created_at: DateTime<Utc>,
}

/// an org plus the caller's role in it, returned from `/orgs/me`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgMembershipView {
    pub org: Organization,
    pub role: OrgRole,
}

// ---- request/response DTOs ----

#[derive(Debug, Clone, Deserialize)]
pub struct CreateOrgRequest {
    pub name: String,
    /// optional explicit slug; derived from `name` when omitted.
    #[serde(default)]
    pub slug: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateOrgRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub disabled: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AddOrgMemberRequest {
    pub user_id: Uuid,
    pub role: OrgRole,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateOrgMemberRequest {
    pub role: OrgRole,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SwitchOrgRequest {
    pub org_id: Uuid,
}

/// the active-org context returned after a switch, with a re-issued access token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgContextResponse {
    pub access_token: String,
    /// access-token lifetime in seconds.
    pub expires_in: i64,
    pub org: Organization,
    pub role: OrgRole,
}

/// derive a url/label-safe slug from a display name: lowercase, non-alphanumerics to hyphens,
/// collapsed and trimmed. empty input yields an empty string (callers should reject that).
pub fn slugify(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut prev_hyphen = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_hyphen = false;
        } else if !prev_hyphen && !out.is_empty() {
            out.push('-');
            prev_hyphen = true;
        }
    }
    out.trim_end_matches('-').to_string()
}
