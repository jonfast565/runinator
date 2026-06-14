//! authentication primitives shared by the web service and tooling: password hashing, api-key and
//! refresh-token generation/hashing, and jwt access tokens. this crate is transport- and
//! storage-agnostic (no axum, no database); the request-gating middleware lives in `runinator-ws`
//! and persistence bootstrap lives in `runinator-database`.

use argon2::Argon2;
use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::Utc;
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use rand::RngCore;
use runinator_models::auth::{ApiKeyRecord, AuthContext, Claims, PrincipalKind};
use std::future::Future;
use uuid::Uuid;

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

/// a freshly generated api key: `secret` is shown to the caller once; `key_hash` is stored.
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

// ---- random bytes / tokens / api keys ----

/// cryptographically random bytes (e.g. for the signing secret).
pub fn random_secret(len: usize) -> Vec<u8> {
    let mut buf = vec![0u8; len];
    rand::thread_rng().fill_bytes(&mut buf);
    buf
}

/// sha256 of a secret, base64url-encoded. used to store api keys and refresh tokens at rest.
pub fn hash_secret(secret: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    URL_SAFE_NO_PAD.encode(hasher.finalize())
}

/// mint an api key. wire form is `<prefix>.<secret>`; we store the prefix (for lookup) and the
/// sha256 of the whole presented string.
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

// ---- jwt access tokens ----

/// issue an access token for a user. returns the token and its expiry (unix seconds).
pub fn issue_access_token(
    config: &AuthConfig,
    user_id: Uuid,
    is_admin: bool,
) -> Result<(String, i64), String> {
    let now = Utc::now().timestamp();
    let exp = now + config.access_ttl_secs;
    let claims = Claims {
        sub: user_id.to_string(),
        adm: is_admin,
        iat: now,
        exp,
        jti: Uuid::new_v4().to_string(),
    };
    encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(&config.jwt_secret),
    )
    .map(|token| (token, exp))
    .map_err(|err| err.to_string())
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

/// the persistence the auth layer needs to verify api keys. implemented in the web service over the
/// database, so this crate can host the resolution logic without depending on a concrete database.
pub trait CredentialStore {
    fn api_key_by_prefix(
        &self,
        prefix: String,
    ) -> impl Future<Output = Option<ApiKeyRecord>> + Send;

    fn touch_api_key(&self, id: Uuid, last_used_at: i64) -> impl Future<Output = ()> + Send;
}

/// resolve a presented credential to a principal: try it as a jwt first, then as a
/// `<prefix>.<secret>` api key looked up through the [`CredentialStore`].
pub async fn resolve_credential<S: CredentialStore>(
    config: &AuthConfig,
    store: &S,
    presented: &str,
) -> Option<AuthContext> {
    if let Some(claims) = verify_access_token(config, presented) {
        return Some(AuthContext {
            principal_id: claims.sub.parse::<Uuid>().ok(),
            is_admin: claims.adm,
            kind: PrincipalKind::User,
        });
    }
    let prefix = presented.split('.').next()?.to_string();
    let record = store.api_key_by_prefix(prefix).await?;
    if record.key.disabled {
        return None;
    }
    if let Some(expires_at) = record.key.expires_at {
        if expires_at < Utc::now() {
            return None;
        }
    }
    if hash_secret(presented) != record.key_hash {
        return None;
    }
    if let Some(id) = record.key.id {
        store.touch_api_key(id, Utc::now().timestamp()).await;
    }
    Some(AuthContext {
        principal_id: record.key.user_id,
        is_admin: record.is_admin,
        kind: PrincipalKind::Service,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> AuthConfig {
        AuthConfig {
            enabled: true,
            jwt_secret: b"test-secret-bytes".to_vec(),
            jwt_secret_previous: None,
            access_ttl_secs: 3600,
            refresh_ttl_secs: 86400,
        }
    }

    #[test]
    fn password_hash_round_trips() {
        let hash = hash_password("hunter2").expect("hash");
        assert!(verify_password("hunter2", &hash));
        assert!(!verify_password("wrong", &hash));
    }

    #[test]
    fn access_token_round_trips_and_carries_admin() {
        let cfg = config();
        let user_id = Uuid::new_v4();
        let (token, _exp) = issue_access_token(&cfg, user_id, true).expect("issue");
        let claims = verify_access_token(&cfg, &token).expect("verify");
        assert_eq!(claims.sub, user_id.to_string());
        assert!(claims.adm);
    }

    #[test]
    fn access_token_rejected_under_wrong_secret() {
        let (token, _) = issue_access_token(&config(), Uuid::new_v4(), false).expect("issue");
        let other = AuthConfig {
            jwt_secret: b"different-secret".to_vec(),
            ..config()
        };
        assert!(verify_access_token(&other, &token).is_none());
    }

    #[test]
    fn rotated_token_verifies_against_previous_secret() {
        // a token minted before rotation (signed with the old secret).
        let old = config();
        let (token, _) = issue_access_token(&old, Uuid::new_v4(), false).expect("issue");

        // after rotation the old secret moves to the previous slot; new tokens use a fresh primary.
        let rotated = AuthConfig {
            jwt_secret: b"new-primary-secret".to_vec(),
            jwt_secret_previous: Some(old.jwt_secret.clone()),
            ..config()
        };
        assert!(
            verify_access_token(&rotated, &token).is_some(),
            "pre-rotation token must stay valid during the overlap window"
        );

        // once the previous secret is dropped the old token is rejected.
        let retired = AuthConfig {
            jwt_secret_previous: None,
            ..rotated
        };
        assert!(verify_access_token(&retired, &token).is_none());
    }

    #[test]
    fn api_key_hash_matches_only_the_issued_secret() {
        let key = new_api_key();
        assert_eq!(hash_secret(&key.secret), key.key_hash);
        assert_ne!(hash_secret("prefix.bogus"), key.key_hash);
        assert!(key.secret.starts_with(&key.prefix));
    }
}
