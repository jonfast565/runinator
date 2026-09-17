use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const ENVELOPE_PREFIX: &[u8] = b"runinator-secret:v1:";

/// stable, bounded identity for one secret-expiry warning occurrence.
pub fn secret_expiry_occurrence(
    scope: &str,
    name: &str,
    expires_at: DateTime<Utc>,
    warning_seconds: i64,
) -> String {
    let mut digest = Sha256::new();
    digest.update((scope.len() as u64).to_be_bytes());
    digest.update(scope.as_bytes());
    digest.update((name.len() as u64).to_be_bytes());
    digest.update(name.as_bytes());
    digest.update(expires_at.timestamp().to_be_bytes());
    digest.update(warning_seconds.to_be_bytes());
    let digest = digest.finalize();
    hex::encode(digest)
}

#[cfg(test)]
#[path = "stored_secret_tests.rs"]
mod tests;

mod stored_secret;
pub use stored_secret::StoredSecret;

mod stored_secret_envelope;
use stored_secret_envelope::StoredSecretEnvelope;
