use runinator_auth::AuthConfig;
use runinator_models::auth::Claims;

// env vars configuring the broker's bearer-token gate. when no secret is set the broker stays open
// (the local/dev default); the supervisor stack is unaffected.
const SECRET_ENV: &str = "RUNINATOR_BROKER_AUTH_SECRET";
const SECRET_PREVIOUS_ENV: &str = "RUNINATOR_BROKER_AUTH_SECRET_PREVIOUS";

/// verifies bearer tokens presented to the broker against a shared HS256 secret (the same secret the
/// web service signs with). a token's `rid` claim, when present, scopes it to one worker replica so
/// the broker can authorize targeting without a registry lookup.
#[derive(Clone)]
pub struct BrokerAuth {
    config: AuthConfig,
}

impl BrokerAuth {
    /// build from env: returns `None` when no secret is configured, leaving the broker open.
    pub fn from_env() -> Option<Self> {
        let secret = std::env::var(SECRET_ENV)
            .ok()
            .filter(|value| !value.trim().is_empty())?;
        let previous = std::env::var(SECRET_PREVIOUS_ENV)
            .ok()
            .filter(|value| !value.trim().is_empty());
        Some(Self::new(
            secret.into_bytes(),
            previous.map(String::into_bytes),
        ))
    }

    pub fn new(secret: Vec<u8>, previous: Option<Vec<u8>>) -> Self {
        Self {
            config: AuthConfig {
                enabled: true,
                jwt_secret: secret,
                jwt_secret_previous: previous,
                access_ttl_secs: 0,
                refresh_ttl_secs: 0,
            },
        }
    }

    /// verify a bearer token, returning its claims on success.
    pub fn verify(&self, token: &str) -> Option<Claims> {
        runinator_auth::verify_access_token(&self.config, token)
    }
}

/// the authenticated identity attached to a request: `None` when broker auth is disabled (every
/// request is anonymous) — handlers treat that as "no authz constraints".
#[derive(Clone)]
pub struct AuthIdentity(pub Option<Claims>);

#[cfg(test)]
#[path = "auth_tests.rs"]
mod tests;
