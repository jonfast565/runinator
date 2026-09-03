//! identity and access: users, credentials, API keys, sessions, teams, and resource grants.
//!
//! one of the role traits `DatabaseImpl` composes. bound on this directly when a caller only
//! needs this slice of the store.

use std::future::Future;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use runinator_models::{
    auth::{
        AgentEnrollmentToken, AgentEnrollmentTokenRecord, ApiKey, ApiKeyRecord, AuthSession, Grant,
        LocalCredential, Team, User,
    },
    errors::SendableError,
    rbac::PlatformRole,
};

/// Core persistence operations for Runinator.
/// Identity and access: users, credentials, API keys, sessions, teams, and resource grants.
pub trait AuthStore: Send + Sync + 'static {
    // ---- auth: users, identities, API keys, sessions ----

    /// Create a user and, when `password_hash` is set, a matching local identity.
    fn create_user(
        &self,
        username: String,
        email: Option<String>,
        password_hash: Option<String>,
    ) -> impl Future<Output = Result<User, SendableError>> + Send;

    /// Atomically create a user, optional local identity, and mandatory initial platform role.
    fn create_user_with_platform_role(
        &self,
        username: String,
        email: Option<String>,
        password_hash: Option<String>,
        role: PlatformRole,
        created_by: Option<Uuid>,
    ) -> impl Future<Output = Result<User, SendableError>> + Send;

    /// Fetch a user by id.
    fn fetch_user(
        &self,
        id: Uuid,
    ) -> impl Future<Output = Result<Option<User>, SendableError>> + Send;

    /// Fetch a user by username.
    fn fetch_user_by_username(
        &self,
        username: String,
    ) -> impl Future<Output = Result<Option<User>, SendableError>> + Send;

    /// Resolve a local login: the user plus the stored argon2 hash for `username`.
    fn fetch_local_credential(
        &self,
        username: String,
    ) -> impl Future<Output = Result<Option<LocalCredential>, SendableError>> + Send;

    /// List all users.
    fn list_users(&self) -> impl Future<Output = Result<Vec<User>, SendableError>> + Send;

    /// Count users (used to decide whether to seed a bootstrap admin).
    fn count_users(&self) -> impl Future<Output = Result<i64, SendableError>> + Send;

    /// Patch a user's mutable fields (None leaves a field unchanged).
    fn update_user(
        &self,
        id: Uuid,
        email: Option<Option<String>>,
        disabled: Option<bool>,
    ) -> impl Future<Output = Result<User, SendableError>> + Send;

    /// Set (upsert) a user's local password hash.
    fn set_local_password(
        &self,
        user_id: Uuid,
        password_hash: String,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Delete a user and their identities/sessions.
    fn delete_user(&self, id: Uuid) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Create an API key from a fully-formed record (caller supplies the hash).
    fn create_api_key(
        &self,
        record: ApiKeyRecord,
    ) -> impl Future<Output = Result<ApiKey, SendableError>> + Send;

    /// Fetch an API key (incl. hash) by id for administration.
    fn fetch_api_key(
        &self,
        id: Uuid,
    ) -> impl Future<Output = Result<Option<ApiKeyRecord>, SendableError>> + Send;

    /// Fetch an API key (incl. hash) by its public prefix for verification.
    fn fetch_api_key_by_prefix(
        &self,
        prefix: String,
    ) -> impl Future<Output = Result<Option<ApiKeyRecord>, SendableError>> + Send;

    /// List API keys, optionally scoped to one owner.
    fn list_api_keys(
        &self,
        user_id: Option<Uuid>,
    ) -> impl Future<Output = Result<Vec<ApiKey>, SendableError>> + Send;

    /// Disable (revoke) an API key.
    fn revoke_api_key(&self, id: Uuid) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Update API key metadata.
    fn update_api_key(
        &self,
        id: Uuid,
        name: Option<String>,
        expires_at: Option<Option<DateTime<Utc>>>,
        disabled: Option<bool>,
    ) -> impl Future<Output = Result<ApiKey, SendableError>> + Send;

    /// Record an API key's last-used timestamp (best effort).
    fn touch_api_key(
        &self,
        id: Uuid,
        last_used_at: i64,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    fn create_agent_enrollment_token(
        &self,
        record: AgentEnrollmentTokenRecord,
    ) -> impl Future<Output = Result<AgentEnrollmentToken, SendableError>> + Send;

    fn fetch_agent_enrollment_token(
        &self,
        token_id: String,
    ) -> impl Future<Output = Result<Option<AgentEnrollmentTokenRecord>, SendableError>> + Send;

    fn list_agent_enrollment_tokens(
        &self,
    ) -> impl Future<Output = Result<Vec<AgentEnrollmentToken>, SendableError>> + Send;

    fn delete_agent_enrollment_token(
        &self,
        token_id: String,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    fn purge_expired_enrollment_tokens(
        &self,
        before: DateTime<Utc>,
    ) -> impl Future<Output = Result<u64, SendableError>> + Send;

    /// atomically consume a still-live single-use token and mint its agent credential.
    fn consume_enrollment_token_and_create_api_key(
        &self,
        token_id: String,
        record: ApiKeyRecord,
        consumed_at: DateTime<Utc>,
    ) -> impl Future<Output = Result<Option<ApiKey>, SendableError>> + Send;

    /// Create a refresh session.
    fn create_session(
        &self,
        session: AuthSession,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Fetch a non-revoked session by its refresh-token hash.
    fn fetch_session_by_hash(
        &self,
        refresh_token_hash: String,
    ) -> impl Future<Output = Result<Option<AuthSession>, SendableError>> + Send;

    /// Atomically consume one refresh from a live session, revoking it so a concurrent refresh
    /// cannot reuse the same refresh token. Returns false when the session is already revoked or
    /// has reached the configured refresh budget.
    fn consume_session_refresh(
        &self,
        id: Uuid,
        max_refreshes: i64,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    fn fetch_session(
        &self,
        id: Uuid,
    ) -> impl Future<Output = Result<Option<AuthSession>, SendableError>> + Send;

    /// List active, unexpired refresh sessions for one user.
    fn list_user_sessions(
        &self,
        user_id: Uuid,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<Vec<AuthSession>, SendableError>> + Send;

    /// Coarsely update activity metadata when the persisted value is older than `stale_before`.
    fn touch_session_activity(
        &self,
        id: Uuid,
        seen_at: DateTime<Utc>,
        stale_before: DateTime<Utc>,
        user_agent: Option<String>,
        ip_address: Option<String>,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Revoke a single session.
    fn revoke_session(&self, id: Uuid) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Revoke every session for a user (logout-all / password change).
    fn revoke_user_sessions(
        &self,
        user_id: Uuid,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Revoke every session for a user except the session currently serving the request.
    fn revoke_user_sessions_except(
        &self,
        user_id: Uuid,
        current_session_id: Uuid,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    // ---- authz: teams + resource grants ----

    /// Create a team.
    fn create_team(
        &self,
        name: String,
        scope: runinator_models::rbac::ScopeRef,
    ) -> impl Future<Output = Result<Team, SendableError>> + Send;

    /// Rename a team.
    fn update_team(
        &self,
        id: Uuid,
        name: String,
    ) -> impl Future<Output = Result<Team, SendableError>> + Send;

    /// Fetch one team by id.
    fn fetch_team(
        &self,
        id: Uuid,
    ) -> impl Future<Output = Result<Option<Team>, SendableError>> + Send;

    /// List all teams. Callers must filter this platform-wide administrative view to an
    /// authorized tenant before returning it to a user.
    fn list_teams(&self) -> impl Future<Output = Result<Vec<Team>, SendableError>> + Send;

    /// Delete a team and its memberships.
    fn delete_team(&self, id: Uuid) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Add a user to a team (idempotent).
    fn add_team_member(
        &self,
        team_id: Uuid,
        user_id: Uuid,
        role: runinator_models::rbac::TeamRole,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Remove a user from a team.
    fn remove_team_member(
        &self,
        team_id: Uuid,
        user_id: Uuid,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// The team ids a user belongs to (used to resolve effective permissions).
    fn list_user_team_ids(
        &self,
        user_id: Uuid,
    ) -> impl Future<Output = Result<Vec<Uuid>, SendableError>> + Send;

    /// The teams a user belongs to.
    fn list_user_teams(
        &self,
        user_id: Uuid,
    ) -> impl Future<Output = Result<Vec<Team>, SendableError>> + Send;

    /// The users assigned to a team.
    fn list_team_members(
        &self,
        team_id: Uuid,
    ) -> impl Future<Output = Result<Vec<User>, SendableError>> + Send;

    /// Create or update (by resource+principal) a grant.
    fn create_grant(
        &self,
        grant: Grant,
    ) -> impl Future<Output = Result<Grant, SendableError>> + Send;

    /// Revoke a grant by id.
    fn revoke_grant(
        &self,
        grant_id: Uuid,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// All grants on a resource.
    fn list_grants(
        &self,
        resource_type: String,
        resource_id: Uuid,
    ) -> impl Future<Output = Result<Vec<Grant>, SendableError>> + Send;

    /// A user's direct grants of a resource type (for visibility scoping).
    fn list_user_grants(
        &self,
        resource_type: String,
        user_id: Uuid,
    ) -> impl Future<Output = Result<Vec<Grant>, SendableError>> + Send;

    /// A team's grants of a resource type (for visibility scoping).
    fn list_team_grants(
        &self,
        resource_type: String,
        team_id: Uuid,
    ) -> impl Future<Output = Result<Vec<Grant>, SendableError>> + Send;
}
