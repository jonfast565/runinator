use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const ENVELOPE_PREFIX: &[u8] = b"runinator-secret:v1:";

/// a settings-store secret after its encrypted payload has been opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSecret {
    pub value: String,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredSecretEnvelope {
    value: String,
    expires_at: DateTime<Utc>,
}

impl StoredSecret {
    pub fn new(value: String, expires_at: Option<DateTime<Utc>>) -> Self {
        Self { value, expires_at }
    }

    /// encode a secret for encryption and persistence. secrets without metadata retain the legacy
    /// raw-string representation so existing stores and consumers remain compatible.
    pub fn encode(&self) -> Result<Vec<u8>, serde_json::Error> {
        let Some(expires_at) = self.expires_at else {
            return Ok(self.value.as_bytes().to_vec());
        };
        let envelope = StoredSecretEnvelope {
            value: self.value.clone(),
            expires_at,
        };
        let mut encoded = ENVELOPE_PREFIX.to_vec();
        encoded.extend(serde_json::to_vec(&envelope)?);
        Ok(encoded)
    }

    /// decode an opened settings payload, treating legacy or malformed envelopes as raw secrets.
    pub fn decode(bytes: &[u8]) -> Self {
        let Some(payload) = bytes.strip_prefix(ENVELOPE_PREFIX) else {
            return Self::new(String::from_utf8_lossy(bytes).into_owned(), None);
        };
        match serde_json::from_slice::<StoredSecretEnvelope>(payload) {
            Ok(envelope) => Self::new(envelope.value, Some(envelope.expires_at)),
            Err(_) => Self::new(String::from_utf8_lossy(bytes).into_owned(), None),
        }
    }
}

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
    format!("{digest:x}")
}

#[cfg(test)]
#[path = "stored_secret_tests.rs"]
mod tests;
