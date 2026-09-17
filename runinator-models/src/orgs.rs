// organization (tenant) domain/wire types. an org owns workflows/runs/resources; users belong to
// many orgs, each with a role, and act within one active org at a time. authorization within an org
// Uses the `OrgRole` ladder beneath the platform role hierarchy.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::validation::{SHORT_TEXT_MAX, Validate, ValidationError, optional_text, required_text};

/// the per-org role ladder. higher variants subsume lower ones (owner ⊇ admin ⊇ member).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrgRole {
    Member,
    Operator,
    Admin,
    Owner,
}

impl OrgRole {
    pub const fn as_str(self) -> &'static str {
        match self {
            OrgRole::Member => "member",
            OrgRole::Operator => "operator",
            OrgRole::Admin => "admin",
            OrgRole::Owner => "owner",
        }
    }

    pub fn from_str_lossy(raw: &str) -> Option<Self> {
        match raw {
            "member" => Some(OrgRole::Member),
            "operator" => Some(OrgRole::Operator),
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

// ---- request/response DTOs ----

/// derive a URL/label-safe slug from a display name: lowercase, non-alphanumerics to hyphens,
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

mod organization;
pub use organization::Organization;

mod org_membership;
pub use org_membership::OrgMembership;

mod org_membership_view;
pub use org_membership_view::OrgMembershipView;

mod create_org_request;
pub use create_org_request::CreateOrgRequest;

mod update_org_request;
pub use update_org_request::UpdateOrgRequest;

mod add_org_member_request;
pub use add_org_member_request::AddOrgMemberRequest;

mod update_org_member_request;
pub use update_org_member_request::UpdateOrgMemberRequest;

mod switch_org_request;
pub use switch_org_request::SwitchOrgRequest;

mod org_context_response;
pub use org_context_response::OrgContextResponse;

mod platform_context_response;
pub use platform_context_response::PlatformContextResponse;
