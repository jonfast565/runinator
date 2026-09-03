//! Shared authentication helpers for the web service and tools. They cover password hashing,
//! API keys, refresh tokens, and JWT access tokens. This crate does not depend on a transport or
//! a database. Request gating lives in `runinator-ws`; persistence setup lives in
//! `runinator-database`.

use argon2::Argon2;
use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::Utc;
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use rand::RngCore;
use runinator_models::auth::{
    ApiKeyRecord, AuthContext, AuthSession, Claims, PrincipalKind, ReplicaClaims, User,
};
use runinator_models::rbac::{PlatformRole, Role, RoleAssignment, ServiceAccount};
use std::future::Future;
use uuid::Uuid;

pub mod enroll;

/// raw auth options from the CLI/env, resolved into an [`AuthConfig`] at startup.
#[derive(Debug, Clone, Default)]
pub struct AuthOptions {
    pub enabled: bool,
    pub access_ttl_secs: i64,
    pub refresh_ttl_secs: i64,
}

/// runtime auth configuration shared across handlers and the middleware.
#[derive(Debug, Clone)]
pub struct AuthConfig {
    pub enabled: bool,
    /// primary signing secret: every freshly issued token is signed with this.
    pub jwt_secret: Vec<u8>,
    /// optional previous signing secret accepted on verify during a rotation overlap window. tokens
    /// are never signed with it; it only keeps pre-rotation tokens valid until they expire.
    pub jwt_secret_previous: Option<Vec<u8>>,
    pub access_ttl_secs: i64,
    pub refresh_ttl_secs: i64,
}

/// A new API key. Show `secret` to the caller once and store `key_hash`.
pub struct NewApiKey {
    pub prefix: String,
    pub secret: String,
    pub key_hash: String,
}

// ---- password hashing (argon2) ----

pub fn hash_password(password: &str) -> Result<String, String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|err| err.to_string())
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

/// run a verification against a fixed dummy hash and discard the result. callers use this on the
/// "no such user" login path so a failed login costs the same argon2 work whether or not the
/// username exists, defeating username enumeration via response timing.
pub fn dummy_verify(password: &str) {
    static DUMMY_HASH: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    let hash = DUMMY_HASH.get_or_init(|| {
        // the password behind this hash is irrelevant; we only need a valid, stable phc string so
        // the verify performs the same work as a real one.
        hash_password("runinator-dummy-verify").unwrap_or_default()
    });
    let _ = verify_password(password, hash);
}

// ---- random bytes / tokens / API keys ----

/// cryptographically random bytes (e.g. for the signing secret).
pub fn random_secret(len: usize) -> Vec<u8> {
    let mut buf = vec![0u8; len];
    rand::thread_rng().fill_bytes(&mut buf);
    buf
}

/// SHA-256 of a secret, encoded with base64url. Store this for API keys and refresh tokens.
pub fn hash_secret(secret: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    URL_SAFE_NO_PAD.encode(hasher.finalize())
}

/// Create an API key. Its wire form is `<prefix>.<secret>`.
/// Store the prefix for lookup and the SHA-256 hash for verification.
pub fn new_api_key() -> NewApiKey {
    let prefix = URL_SAFE_NO_PAD.encode(random_secret(6));
    let body = URL_SAFE_NO_PAD.encode(random_secret(32));
    let secret = format!("{prefix}.{body}");
    let key_hash = hash_secret(&secret);
    NewApiKey {
        prefix,
        key_hash,
        secret,
    }
}

/// a refresh token (returned to the client) and its stored hash.
pub fn new_refresh_token() -> (String, String) {
    let token = URL_SAFE_NO_PAD.encode(random_secret(32));
    let hash = hash_secret(&token);
    (token, hash)
}

// ---- JWT access tokens ----

/// Issue an access token carrying identity and selected context only. Roles resolve live.
pub fn issue_access_token(
    config: &AuthConfig,
    user_id: Uuid,
    session_id: Uuid,
    org: Option<Uuid>,
) -> Result<(String, i64), String> {
    let now = Utc::now().timestamp();
    let exp = now + config.access_ttl_secs;
    let claims = Claims {
        sub: user_id.to_string(),
        sid: session_id.to_string(),
        iat: now,
        exp,
        jti: Uuid::new_v4().to_string(),
        org: org.map(|id| id.to_string()),
    };
    encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(&config.jwt_secret),
    )
    .map(|token| (token, exp))
    .map_err(|err| err.to_string())
}

/// issue a replica-scoped broker token: a JWT whose `rid` claim pins it to one worker replica. the
/// broker verifies it and refuses any consumer profile presenting a different replica id, so a client
/// cannot impersonate another user's desktop worker. returns the token and its expiry.
pub fn issue_replica_token(
    config: &AuthConfig,
    user_id: Uuid,
    replica_id: Uuid,
) -> Result<(String, i64), String> {
    let now = Utc::now().timestamp();
    let exp = now + config.access_ttl_secs;
    let claims = ReplicaClaims {
        sub: user_id.to_string(),
        iat: now,
        exp,
        jti: Uuid::new_v4().to_string(),
        rid: replica_id.to_string(),
    };
    encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(&config.jwt_secret),
    )
    .map(|token| (token, exp))
    .map_err(|err| err.to_string())
}

pub fn verify_replica_token(config: &AuthConfig, token: &str) -> Option<ReplicaClaims> {
    if let Some(claims) = verify_replica_with_secret(&config.jwt_secret, token) {
        return Some(claims);
    }
    config
        .jwt_secret_previous
        .as_deref()
        .and_then(|previous| verify_replica_with_secret(previous, token))
}

fn verify_replica_with_secret(secret: &[u8], token: &str) -> Option<ReplicaClaims> {
    decode::<ReplicaClaims>(
        token,
        &DecodingKey::from_secret(secret),
        &Validation::new(Algorithm::HS256),
    )
    .ok()
    .map(|data| data.claims)
}

/// verify and decode an access token; `None` on any failure (bad signature, expired, malformed).
/// during key rotation the primary secret is tried first, then the optional previous secret, so
/// tokens signed before the rotation stay valid until they expire.
pub fn verify_access_token(config: &AuthConfig, token: &str) -> Option<Claims> {
    if let Some(claims) = verify_with_secret(&config.jwt_secret, token) {
        return Some(claims);
    }
    config
        .jwt_secret_previous
        .as_deref()
        .and_then(|previous| verify_with_secret(previous, token))
}

fn verify_with_secret(secret: &[u8], token: &str) -> Option<Claims> {
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret),
        &Validation::new(Algorithm::HS256),
    )
    .ok()
    .map(|data| data.claims)
}

// ---- credential resolution (db-agnostic via a trait) ----

/// Persistence needed to verify API keys. The web service implements this with a database,
/// while this crate keeps the resolution logic independent of a concrete database.
pub trait CredentialStore {
    fn api_key_by_prefix(
        &self,
        prefix: String,
    ) -> impl Future<Output = Option<ApiKeyRecord>> + Send;

    fn touch_api_key(&self, id: Uuid, last_used_at: i64) -> impl Future<Output = ()> + Send;

    fn user_by_id(&self, id: Uuid) -> impl Future<Output = Option<User>> + Send;

    fn session_by_id(&self, id: Uuid) -> impl Future<Output = Option<AuthSession>> + Send;

    fn service_account_by_id(
        &self,
        id: Uuid,
    ) -> impl Future<Output = Option<ServiceAccount>> + Send;

    fn role_assignments(
        &self,
        kind: PrincipalKind,
        id: Uuid,
    ) -> impl Future<Output = Option<Vec<RoleAssignment>>> + Send;

    /// Resolve an organization selected by a token or scoped API key. Authentication must reject
    /// disabled tenants before a request reaches any resource handler.
    fn organization_by_id(
        &self,
        id: Uuid,
    ) -> impl Future<Output = Option<runinator_models::orgs::Organization>> + Send;
}

/// Resolve a credential to a principal. Try a JWT first, then look up a
/// `<prefix>.<secret>` API key through [`CredentialStore`].
pub async fn resolve_credential<S: CredentialStore>(
    config: &AuthConfig,
    store: &S,
    presented: &str,
) -> Option<AuthContext> {
    if let Some(claims) = verify_access_token(config, presented) {
        let principal_id = claims.sub.parse::<Uuid>().ok()?;
        let session_id = claims.sid.parse::<Uuid>().ok()?;
        let session = store.session_by_id(session_id).await?;
        if session.revoked || session.expires_at < Utc::now() || session.user_id != principal_id {
            return None;
        }
        let user = store.user_by_id(principal_id).await?;
        if user.disabled {
            return None;
        }
        let assignments = store
            .role_assignments(PrincipalKind::User, principal_id)
            .await?;
        let platform_role = platform_role(&assignments);
        let org_id = claims.org.as_deref().and_then(|id| id.parse::<Uuid>().ok());
        if let Some(org_id) = org_id
            && platform_role != Some(PlatformRole::Admin)
            && !assignments.iter().any(|assignment| {
                assignment.scope.kind == runinator_models::rbac::ScopeKind::Organization
                    && assignment.scope.id == Some(org_id)
            })
        {
            return None;
        }
        if let Some(org_id) = org_id
            && store.organization_by_id(org_id).await?.disabled
        {
            return None;
        }
        return Some(AuthContext {
            principal_id: Some(principal_id),
            session_id: Some(session_id),
            kind: PrincipalKind::User,
            platform_role,
            assignments,
            system_role: None,
            action_ceiling: Vec::new(),
            org_id,
        });
    }
    let prefix = presented.split('.').next()?.to_string();
    let record = store.api_key_by_prefix(prefix).await?;
    if record.key.disabled {
        return None;
    }
    if let Some(expires_at) = record.key.expires_at
        && expires_at < Utc::now()
    {
        return None;
    }
    if hash_secret(presented) != record.key_hash {
        return None;
    }
    match record.key.principal_kind {
        PrincipalKind::User if store.user_by_id(record.key.principal_id).await?.disabled => {
            return None;
        }
        PrincipalKind::Service
            if store
                .service_account_by_id(record.key.principal_id)
                .await?
                .disabled =>
        {
            return None;
        }
        _ => {}
    }
    let assignments = store
        .role_assignments(record.key.principal_kind, record.key.principal_id)
        .await?;
    if let Some(org_id) = record.key.org_id
        && store.organization_by_id(org_id).await?.disabled
    {
        return None;
    }
    if let Some(id) = record.key.id {
        store.touch_api_key(id, Utc::now().timestamp()).await;
    }
    Some(AuthContext {
        principal_id: Some(record.key.principal_id),
        session_id: None,
        kind: record.key.principal_kind,
        platform_role: platform_role(&assignments),
        assignments,
        system_role: record.key.system_role,
        action_ceiling: record.key.action_ceiling,
        org_id: record.key.org_id,
    })
}

fn platform_role(assignments: &[RoleAssignment]) -> Option<PlatformRole> {
    assignments
        .iter()
        .filter_map(|assignment| match assignment.role {
            Role::Platform(role) => Some(role),
            _ => None,
        })
        .max()
}

#[cfg(test)]
mod tests;
