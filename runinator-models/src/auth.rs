// authentication & identity domain/wire types. authorization (grants/teams) lands in a later phase;
// phase 1 carries only a global `is_admin` flag plus service api keys.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// the local-password identity provider tag. future SSO providers use `"oidc:<issuer>"`.
pub const PROVIDER_LOCAL: &str = "local";

// ---- resource-based authorization (phase 2) ----

/// the permission ladder for a resource grant. higher variants subsume lower ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    View,
    Run,
    Edit,
    Own,
}

impl Permission {
    pub fn as_str(&self) -> &'static str {
        match self {
            Permission::View => "view",
            Permission::Run => "run",
            Permission::Edit => "edit",
            Permission::Own => "own",
        }
    }

    pub fn from_str_lossy(raw: &str) -> Option<Self> {
        match raw {
            "view" => Some(Permission::View),
            "run" => Some(Permission::Run),
            "edit" => Some(Permission::Edit),
            "own" => Some(Permission::Own),
            _ => None,
        }
    }

    /// true when this permission is at least as strong as `required`.
    pub fn allows(self, required: Permission) -> bool {
        self >= required
    }
}

/// the kind of resource a grant targets. workflows are the primary owned object; their runs and
/// sub-resources inherit the parent workflow's permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceType {
    Workflow,
}

impl ResourceType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ResourceType::Workflow => "workflow",
        }
    }
}

/// whether a grant is held by a user or a team.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalType {
    User,
    Team,
}

impl PrincipalType {
    pub fn as_str(&self) -> &'static str {
        match self {
            PrincipalType::User => "user",
            PrincipalType::Team => "team",
        }
    }

    pub fn from_str_lossy(raw: &str) -> Option<Self> {
        match raw {
            "user" => Some(PrincipalType::User),
            "team" => Some(PrincipalType::Team),
            _ => None,
        }
    }
}

/// a single access grant on a resource.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Grant {
    pub id: Option<Uuid>,
    pub resource_type: ResourceType,
    pub resource_id: Uuid,
    pub principal_type: PrincipalType,
    pub principal_id: Uuid,
    pub permission: Permission,
    pub created_at: DateTime<Utc>,
}

/// a team: a named principal that grants can target, with users as members.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Team {
    pub id: Option<Uuid>,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateTeamRequest {
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AddTeamMemberRequest {
    pub user_id: Uuid,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateGrantRequest {
    pub principal_type: PrincipalType,
    pub principal_id: Uuid,
    pub permission: Permission,
}

/// a user account in wire form. never carries a password hash.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: Option<Uuid>,
    pub username: String,
    #[serde(default)]
    pub email: Option<String>,
    pub is_admin: bool,
    pub disabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// a verified local credential lookup: the user plus the stored argon2 hash to check against.
#[derive(Debug, Clone)]
pub struct LocalCredential {
    pub user: User,
    pub password_hash: String,
}

/// api-key metadata in wire form. never carries the secret or its hash.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    pub id: Option<Uuid>,
    pub name: String,
    #[serde(default)]
    pub user_id: Option<Uuid>,
    pub is_service: bool,
    pub key_prefix: String,
    #[serde(default)]
    pub last_used_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
    pub disabled: bool,
    pub created_at: DateTime<Utc>,
}

/// persistence-facing api-key record: metadata plus the secret hash used to verify a presented key.
#[derive(Debug, Clone)]
pub struct ApiKeyRecord {
    pub key: ApiKey,
    /// resolved admin flag for the principal this key acts as (service keys are admin in phase 1).
    pub is_admin: bool,
    pub key_hash: String,
}

/// a revocable refresh session backing a logged-in user.
#[derive(Debug, Clone)]
pub struct AuthSession {
    pub id: Uuid,
    pub user_id: Uuid,
    pub refresh_token_hash: String,
    pub expires_at: DateTime<Utc>,
    pub revoked: bool,
}

/// jwt access-token claims.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    /// subject: the user id.
    pub sub: String,
    /// admin flag, carried so the middleware needn't re-read the db per request.
    pub adm: bool,
    /// issued-at (unix seconds).
    pub iat: i64,
    /// expiry (unix seconds).
    pub exp: i64,
    /// token id, for future revocation lists.
    pub jti: String,
}

/// how a request was authenticated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrincipalKind {
    User,
    Service,
}

/// the resolved principal for an authenticated request, injected as an axum extension.
#[derive(Debug, Clone)]
pub struct AuthContext {
    pub principal_id: Option<Uuid>,
    pub is_admin: bool,
    pub kind: PrincipalKind,
}

impl AuthContext {
    /// the synthetic admin used when auth is disabled, so existing behavior is unchanged.
    pub fn disabled_admin() -> Self {
        Self {
            principal_id: None,
            is_admin: true,
            kind: PrincipalKind::Service,
        }
    }
}

// ---- request/response DTOs ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfigResponse {
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginResponse {
    pub access_token: String,
    pub refresh_token: String,
    /// access-token lifetime in seconds.
    pub expires_in: i64,
    pub user: User,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshRequest {
    pub refresh_token: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateUserRequest {
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub is_admin: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateUserRequest {
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub is_admin: Option<bool>,
    #[serde(default)]
    pub disabled: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateApiKeyRequest {
    pub name: String,
    #[serde(default)]
    pub is_service: bool,
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
}

/// returned once on creation; `secret` is the only time the raw key is shown.
#[derive(Debug, Clone, Serialize)]
pub struct CreateApiKeyResponse {
    pub api_key: ApiKey,
    pub secret: String,
}
