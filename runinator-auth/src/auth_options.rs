#[allow(unused_imports)]
use super::*;

/// raw auth options from the CLI/env, resolved into an [`AuthConfig`] at startup.
#[derive(Debug, Clone, Default)]
pub struct AuthOptions {
    pub enabled: bool,
    pub access_ttl_secs: i64,
    pub refresh_ttl_secs: i64,
}
