//! identity and access: users, credentials, api keys, sessions, teams, and resource grants.
//!
//! one of the role traits `DatabaseImpl` composes. bound on this directly when a caller only
//! needs this slice of the store.

use std::future::Future;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use runinator_models::{
    auth::{ApiKey, ApiKeyRecord, AuthSession, Grant, LocalCredential, Team, User},
    errors::SendableError,
};

// re-exported here so callers that reach for the contract at its historical path
// (`runinator_database::interfaces::*`) can import both halves from one place.
pub use crate::reducer_store::ReducerStore;

/// Core persistence operations for Runinator.
/// Identity and access: users, credentials, api keys, sessions, teams, and resource grants.
pub trait AuthStore: Send + Sync + 'static {
    // ---- auth: users, identities, api keys, sessions ----

    /// Create a user and, when `password_hash` is set, a matching local identity.
    fn create_user(
        &self,
        username: String,
        email: Option<String>,
        is_admin: bool,
        password_hash: Option<String>,
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
        email: Option<String>,
        is_admin: Option<bool>,
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

    /// Create an api key from a fully-formed record (caller supplies the hash).
    fn create_api_key(
        &self,
        record: ApiKeyRecord,
    ) -> impl Future<Output = Result<ApiKey, SendableError>> + Send;

    /// Fetch an api key (incl. hash) by id for administration.
    fn fetch_api_key(
        &self,
        id: Uuid,
    ) -> impl Future<Output = Result<Option<ApiKeyRecord>, SendableError>> + Send;

    /// Fetch an api key (incl. hash) by its public prefix for verification.
    fn fetch_api_key_by_prefix(
        &self,
        prefix: String,
    ) -> impl Future<Output = Result<Option<ApiKeyRecord>, SendableError>> + Send;

    /// List api keys, optionally scoped to one owner.
    fn list_api_keys(
        &self,
        user_id: Option<Uuid>,
    ) -> impl Future<Output = Result<Vec<ApiKey>, SendableError>> + Send;

    /// Disable (revoke) an api key.
    fn revoke_api_key(&self, id: Uuid) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Update api key metadata.
    fn update_api_key(
        &self,
        id: Uuid,
        name: Option<String>,
        expires_at: Option<Option<DateTime<Utc>>>,
        disabled: Option<bool>,
    ) -> impl Future<Output = Result<ApiKey, SendableError>> + Send;

    /// Record an api key's last-used timestamp (best effort).
    fn touch_api_key(
        &self,
        id: Uuid,
        last_used_at: i64,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

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

    /// Revoke a single session.
    fn revoke_session(&self, id: Uuid) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Revoke every session for a user (logout-all / password change).
    fn revoke_user_sessions(
        &self,
        user_id: Uuid,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    // ---- authz: teams + resource grants ----

    /// Create a team.
    fn create_team(&self, name: String)
    -> impl Future<Output = Result<Team, SendableError>> + Send;

    /// Rename a team.
    fn update_team(
        &self,
        id: Uuid,
        name: String,
    ) -> impl Future<Output = Result<Team, SendableError>> + Send;

    /// List all teams.
    fn list_teams(&self) -> impl Future<Output = Result<Vec<Team>, SendableError>> + Send;

    /// Delete a team and its memberships.
    fn delete_team(&self, id: Uuid) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Add a user to a team (idempotent).
    fn add_team_member(
        &self,
        team_id: Uuid,
        user_id: Uuid,
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
