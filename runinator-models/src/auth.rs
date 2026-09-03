// Authentication and identity wire types. Credentials carry identity only; authorization is
// resolved from live RBAC state.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::rbac::{Action, PlatformRole, RoleAssignment, SystemRole};
use crate::validation::{
    LONG_TEXT_MAX, SHORT_TEXT_MAX, Validate, ValidationError, http_url, identifier, optional_email,
    optional_text, required_text, string_map,
};

/// the local-password identity provider tag. future SSO providers use `"OpenID Connect (OIDC):<issuer>"`.
pub const PROVIDER_LOCAL: &str = "local";

// ---- resource-based authorization ----

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
    Pipeline,
    FunctionPackage,
    ConsoleSession,
    Setting,
    ExecutionProfile,
    OrchestrationAdapter,
    LibraryFile,
    NotificationPolicy,
}

impl ResourceType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ResourceType::Workflow => "workflow",
            ResourceType::Pipeline => "pipeline",
            ResourceType::FunctionPackage => "function_package",
            ResourceType::ConsoleSession => "console_session",
            ResourceType::Setting => "setting",
            ResourceType::ExecutionProfile => "execution_profile",
            ResourceType::OrchestrationAdapter => "orchestration_adapter",
            ResourceType::LibraryFile => "library_file",
            ResourceType::NotificationPolicy => "notification_policy",
        }
    }

    pub fn from_str_lossy(value: &str) -> Option<Self> {
        match value {
            "workflow" => Some(Self::Workflow),
            "pipeline" => Some(Self::Pipeline),
            "function_package" => Some(Self::FunctionPackage),
            "console_session" => Some(Self::ConsoleSession),
            "setting" => Some(Self::Setting),
            "execution_profile" => Some(Self::ExecutionProfile),
            "orchestration_adapter" => Some(Self::OrchestrationAdapter),
            "library_file" => Some(Self::LibraryFile),
            "notification_policy" => Some(Self::NotificationPolicy),
            _ => None,
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
    pub scope: crate::rbac::ScopeRef,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateTeamRequest {
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateTeamRequest {
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AddTeamMemberRequest {
    pub user_id: Uuid,
    pub role: crate::rbac::TeamRole,
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

/// API key metadata in wire form. never carries the secret or its hash.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    pub id: Option<Uuid>,
    pub name: String,
    #[serde(default)]
    pub principal_kind: PrincipalKind,
    pub principal_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_role: Option<SystemRole>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub org_id: Option<Uuid>,
    #[serde(default)]
    pub action_ceiling: Vec<Action>,
    pub key_prefix: String,
    #[serde(default)]
    pub last_used_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
    pub disabled: bool,
    pub created_at: DateTime<Utc>,
}

/// persistence-facing API key record: metadata plus the secret hash used to verify a presented key.
#[derive(Debug, Clone)]
pub struct ApiKeyRecord {
    pub key: ApiKey,
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
    /// Number of successful refresh rotations consumed by this login session.
    pub refresh_count: i64,
    /// The original login time, preserved across refresh-token rotation.
    pub created_at: DateTime<Utc>,
    /// Coarse-grained activity timestamp, updated at most once every few minutes.
    pub last_seen_at: DateTime<Utc>,
    pub user_agent: Option<String>,
    pub ip_address: Option<String>,
}

/// Safe, user-facing view of one refresh session. Credential material is never included.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthSessionSummary {
    pub id: Uuid,
    pub user_agent: Option<String>,
    pub ip_address: Option<String>,
    pub created_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub current: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonalApiKeyScope {
    pub org_id: Option<Uuid>,
    pub name: String,
    pub actions: Vec<Action>,
}

/// JWT access-token claims.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    /// subject: the user id.
    pub sub: String,
    /// backing refresh-session id; access is revoked immediately with the session.
    pub sid: String,
    /// issued-at (unix seconds).
    pub iat: i64,
    /// expiry (unix seconds).
    pub exp: i64,
    /// token id, for future revocation lists.
    pub jti: String,
    /// active organization for this token, when the user has switched into one. absent on tokens
    /// minted before an org was selected, and on service/replica tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub org: Option<String>,
}

/// Broker-only token claims. Kept separate so an ordinary identity JWT can never acquire transport
/// authority by presenting an extra claim.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicaClaims {
    pub sub: String,
    pub iat: i64,
    pub exp: i64,
    pub jti: String,
    pub rid: String,
}

/// how a request was authenticated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalKind {
    #[default]
    User,
    Service,
}

impl PrincipalKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Service => "service",
        }
    }

    pub fn from_str_lossy(raw: &str) -> Option<Self> {
        match raw {
            "user" => Some(Self::User),
            "service" => Some(Self::Service),
            _ => None,
        }
    }
}

/// the resolved principal for an authenticated request, injected as an axum extension.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthContext {
    pub principal_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<Uuid>,
    pub kind: PrincipalKind,
    pub platform_role: Option<PlatformRole>,
    #[serde(default)]
    pub assignments: Vec<RoleAssignment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_role: Option<SystemRole>,
    #[serde(default)]
    pub action_ceiling: Vec<Action>,
    /// active organization for this request, resolved from the token's `org` claim (or an
    /// `X-Org-Id` header for service keys). `None` means platform-global / no org selected.
    pub org_id: Option<Uuid>,
}

impl AuthContext {
    /// the synthetic admin used when auth is disabled, so existing behavior is unchanged.
    pub fn disabled_platform_admin() -> Self {
        Self {
            principal_id: None,
            session_id: None,
            kind: PrincipalKind::Service,
            platform_role: Some(PlatformRole::Admin),
            assignments: Vec::new(),
            system_role: None,
            action_ceiling: Vec::new(),
            org_id: None,
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
    pub assignments: Vec<RoleAssignment>,
    pub effective_actions: Vec<Action>,
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
    #[serde(default = "default_platform_role")]
    pub platform_role: PlatformRole,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateUserRequest {
    #[serde(default)]
    pub email: Option<Option<String>>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub platform_role: Option<PlatformRole>,
    #[serde(default)]
    pub disabled: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateCurrentUserRequest {
    #[serde(default)]
    pub email: Option<Option<String>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreatePersonalApiKeyRequest {
    pub name: String,
    #[serde(default)]
    pub org_id: Option<Uuid>,
    pub action_ceiling: Vec<Action>,
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateApiKeyRequest {
    pub name: String,
    pub principal_kind: PrincipalKind,
    pub principal_id: Uuid,
    #[serde(default)]
    pub system_role: Option<SystemRole>,
    #[serde(default)]
    pub org_id: Option<Uuid>,
    #[serde(default)]
    pub action_ceiling: Vec<Action>,
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
}

fn default_platform_role() -> PlatformRole {
    PlatformRole::Member
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateApiKeyRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub expires_at: Option<Option<DateTime<Utc>>>,
    #[serde(default)]
    pub disabled: Option<bool>,
}

/// returned once on creation; `secret` is the only time the raw key is shown.
#[derive(Debug, Clone, Serialize)]
pub struct CreateApiKeyResponse {
    pub api_key: ApiKey,
    pub secret: String,
}

/// administrative view of a scoped, single-use agent enrollment token. the secret is never
/// returned after creation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentEnrollmentToken {
    pub token_id: String,
    #[serde(default)]
    pub org_id: Option<Uuid>,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    pub service_url: String,
    #[serde(default)]
    pub spki_pin: Option<String>,
    /// When true, redemption creates a non-expiring machine credential. Otherwise the issued
    /// credential expires with this enrollment grant.
    #[serde(default)]
    pub permanent: bool,
    pub expires_at: DateTime<Utc>,
    #[serde(default)]
    pub consumed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub issued_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

/// persistence form; the HMAC secret is sealed with the credential cipher before storage.
#[derive(Debug, Clone)]
pub struct AgentEnrollmentTokenRecord {
    pub token: AgentEnrollmentToken,
    pub sealed_secret: Vec<u8>,
}

/// Durable machine identity created by redeeming an agent enrollment token. Timed credentials
/// expire with their enrollment grant; permanent credentials remain usable until the machine is
/// invalidated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMachineEnrollment {
    pub machine_id: Uuid,
    pub instance_id: String,
    #[serde(default)]
    pub org_id: Option<Uuid>,
    pub permanent: bool,
    pub disabled: bool,
    #[serde(default)]
    pub credential_count: usize,
    #[serde(default)]
    pub active_credential_count: usize,
    #[serde(default)]
    pub enrolled_by: Option<Uuid>,
    pub enrolled_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub last_used_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateAgentEnrollmentTokenRequest {
    /// Redemption deadline in seconds and, for timed enrollment, the issued credential lifetime.
    pub ttl_seconds: u64,
    #[serde(default)]
    pub org_id: Option<Uuid>,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    pub service_url: String,
    /// override for deployments whose LAN announcement URL differs from the public enrollment URL.
    #[serde(default)]
    pub cluster_id: Option<Uuid>,
    #[serde(default)]
    pub spki_pin: Option<String>,
    /// Issue a non-expiring machine credential. Timed access remains the default.
    #[serde(default)]
    pub permanent: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateAgentEnrollmentTokenResponse {
    pub enrollment_token: AgentEnrollmentToken,
    /// shown exactly once.
    pub token: String,
}

/// body authenticated by the enrollment HMAC. labels are a request only; the server rejects any
/// value outside the token's authorized label set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentEnrollmentRequestBody {
    pub instance_id: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrollAgentRequest {
    pub token_id: String,
    pub request_body: AgentEnrollmentRequestBody,
    /// base64url-no-pad HMAC-SHA256 over the canonical JSON request body.
    pub proof: String,
}

impl Validate for CreateTeamRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        required_text("name", &self.name, SHORT_TEXT_MAX)
    }
}

impl Validate for UpdateTeamRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        required_text("name", &self.name, SHORT_TEXT_MAX)
    }
}

impl Validate for AddTeamMemberRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        Ok(())
    }
}

impl Validate for CreateGrantRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        Ok(())
    }
}

impl Validate for LoginRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        required_text("username", &self.username, SHORT_TEXT_MAX)?;
        required_text("password", &self.password, LONG_TEXT_MAX)
    }
}

impl Validate for RefreshRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        required_text("refresh_token", &self.refresh_token, LONG_TEXT_MAX)
    }
}

impl Validate for CreateUserRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        identifier("username", &self.username)?;
        required_text("password", &self.password, LONG_TEXT_MAX)?;
        optional_email("email", self.email.as_deref())
    }
}

impl Validate for UpdateUserRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        optional_email(
            "email",
            self.email.as_ref().and_then(|value| value.as_deref()),
        )?;
        optional_text("password", self.password.as_deref(), LONG_TEXT_MAX)
    }
}

impl Validate for UpdateCurrentUserRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        optional_email(
            "email",
            self.email.as_ref().and_then(|value| value.as_deref()),
        )
    }
}

impl Validate for ChangePasswordRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        required_text("current_password", &self.current_password, LONG_TEXT_MAX)?;
        required_text("new_password", &self.new_password, LONG_TEXT_MAX)
    }
}

impl Validate for CreatePersonalApiKeyRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        required_text("name", &self.name, SHORT_TEXT_MAX)?;
        if self.action_ceiling.is_empty() {
            return Err(ValidationError::new(
                "action_ceiling",
                "must contain at least one action",
            ));
        }
        if self.action_ceiling.len() > 128 {
            return Err(ValidationError::new(
                "action_ceiling",
                "must contain at most 128 actions",
            ));
        }
        Ok(())
    }
}

impl Validate for CreateApiKeyRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        required_text("name", &self.name, SHORT_TEXT_MAX)?;
        if self.action_ceiling.len() > 128 {
            return Err(ValidationError::new(
                "action_ceiling",
                "must contain at most 128 actions",
            ));
        }
        Ok(())
    }
}

impl Validate for UpdateApiKeyRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        optional_text("name", self.name.as_deref(), SHORT_TEXT_MAX)
    }
}

impl Validate for CreateAgentEnrollmentTokenRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        if !(1..=86_400).contains(&self.ttl_seconds) {
            return Err(ValidationError::new(
                "ttl_seconds",
                "must be between 1 and 86400",
            ));
        }
        http_url("service_url", &self.service_url)?;
        string_map("labels", &self.labels, 64)?;
        optional_text("spki_pin", self.spki_pin.as_deref(), SHORT_TEXT_MAX)
    }
}

impl Validate for AgentEnrollmentRequestBody {
    fn validate(&self) -> Result<(), ValidationError> {
        identifier("request_body.instance_id", &self.instance_id)?;
        optional_text(
            "request_body.display_name",
            self.display_name.as_deref(),
            SHORT_TEXT_MAX,
        )?;
        string_map("request_body.labels", &self.labels, 64)
    }
}

impl Validate for EnrollAgentRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        identifier("token_id", &self.token_id)?;
        self.request_body.validate()?;
        required_text("proof", &self.proof, 1024)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrollAgentResponse {
    pub api_key: String,
    pub service_url: String,
    /// Absent for a permanent machine enrollment.
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub org_id: Option<Uuid>,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
}
