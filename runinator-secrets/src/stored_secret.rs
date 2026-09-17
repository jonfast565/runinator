use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

const ENVELOPE_PREFIX: &[u8] = b"runinator-secret:v1:";

/// stable, bounded identity for one secret-expiry warning occurrence.
pub fn secret_expiry_occurrence(
    scope: &str,
    name: &str,
    expires_at: DateTime<Utc>,
    warning_seconds: i64,
) -> String {
    let mut input = Vec::new();
    input.extend_from_slice(&(scope.len() as u64).to_be_bytes());
    input.extend_from_slice(scope.as_bytes());
    input.extend_from_slice(&(name.len() as u64).to_be_bytes());
    input.extend_from_slice(name.as_bytes());
    input.extend_from_slice(&expires_at.timestamp().to_be_bytes());
    input.extend_from_slice(&warning_seconds.to_be_bytes());
    runinator_hash::sha256_hex(&input)
}

#[cfg(test)]
#[path = "stored_secret_tests.rs"]
mod tests;

mod stored_secret;
pub use stored_secret::StoredSecret;

mod stored_secret_envelope;
use stored_secret_envelope::StoredSecretEnvelope;
