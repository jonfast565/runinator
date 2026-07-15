// the named capability catalog: the single documented vocabulary for privileged actions across the
// backend and the command center. capabilities name what an authenticated principal may do; the ws
// authz layer resolves the set a caller holds (see `capabilities_for`) and returns it on `/auth/me`
// so the ui can gate consistently. per-resource grants (see `Permission` in `auth.rs`) are a separate,
// resource-scoped axis and are not enumerated here.

use serde::{Deserialize, Serialize};

/// a named privileged action. platform capabilities are held only by platform admins; org
/// capabilities are held by admins of the caller's active org (and, transitively, platform admins).
/// the wire string is the stable contract shared with the command center.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    // ---- platform capabilities (platform admin only) ----
    /// create, update, and delete user accounts.
    #[serde(rename = "users:manage")]
    UsersManage,
    /// create, update, and delete teams and their membership.
    #[serde(rename = "teams:manage")]
    TeamsManage,
    /// administer api keys beyond one's own (service keys, other users' keys, rotate/revoke).
    #[serde(rename = "apikeys:manage")]
    ApiKeysManage,
    /// read decrypted settings/secrets.
    #[serde(rename = "secrets:read")]
    SecretsRead,
    /// create, update, delete, import, and re-encrypt settings/secrets.
    #[serde(rename = "secrets:write")]
    SecretsWrite,
    /// upsert catalog metadata items.
    #[serde(rename = "catalog:manage")]
    CatalogManage,
    /// read the audit log.
    #[serde(rename = "audit:read")]
    AuditRead,
    /// read the dead-letter queue.
    #[serde(rename = "deadletters:read")]
    DeadLettersRead,
    /// scale or stop platform worker nodes.
    #[serde(rename = "nodes:scale")]
    NodesScale,
    /// import workflow bundles.
    #[serde(rename = "workflows:import")]
    WorkflowsImport,
    /// administer organizations platform-wide (list all, cross-org administration).
    #[serde(rename = "orgs:manage")]
    OrgsManage,
    /// set organization billing quotas.
    #[serde(rename = "billing:manage")]
    BillingManage,
    /// manage platform/admin settings.
    #[serde(rename = "settings:manage")]
    SettingsManage,

    // ---- organization capabilities (admin of the caller's active org, or platform admin) ----
    /// manage membership and roles within the active organization.
    #[serde(rename = "org:members:manage")]
    OrgMembersManage,
    /// scale worker nodes within the active organization.
    #[serde(rename = "org:nodes:scale")]
    OrgNodesScale,
}

impl Capability {
    /// every capability, in catalog order. the ordered source of truth for the full platform set.
    pub const ALL: &'static [Capability] = &[
        Capability::UsersManage,
        Capability::TeamsManage,
        Capability::ApiKeysManage,
        Capability::SecretsRead,
        Capability::SecretsWrite,
        Capability::CatalogManage,
        Capability::AuditRead,
        Capability::DeadLettersRead,
        Capability::NodesScale,
        Capability::WorkflowsImport,
        Capability::OrgsManage,
        Capability::BillingManage,
        Capability::SettingsManage,
        Capability::OrgMembersManage,
        Capability::OrgNodesScale,
    ];

    /// the capabilities an org admin holds in their active org. platform admins hold every capability
    /// (see `ALL`); ordinary members hold none of these.
    pub const ORG_ADMIN: &'static [Capability] =
        &[Capability::OrgMembersManage, Capability::OrgNodesScale];

    pub fn as_str(self) -> &'static str {
        match self {
            Capability::UsersManage => "users:manage",
            Capability::TeamsManage => "teams:manage",
            Capability::ApiKeysManage => "apikeys:manage",
            Capability::SecretsRead => "secrets:read",
            Capability::SecretsWrite => "secrets:write",
            Capability::CatalogManage => "catalog:manage",
            Capability::AuditRead => "audit:read",
            Capability::DeadLettersRead => "deadletters:read",
            Capability::NodesScale => "nodes:scale",
            Capability::WorkflowsImport => "workflows:import",
            Capability::OrgsManage => "orgs:manage",
            Capability::BillingManage => "billing:manage",
            Capability::SettingsManage => "settings:manage",
            Capability::OrgMembersManage => "org:members:manage",
            Capability::OrgNodesScale => "org:nodes:scale",
        }
    }

    pub fn from_str_lossy(raw: &str) -> Option<Self> {
        Capability::ALL
            .iter()
            .copied()
            .find(|cap| cap.as_str() == raw)
    }
}
