//! self-contained, single-use agent enrollment tokens and request proofs.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use uuid::Uuid;

const TOKEN_PREFIX: &str = "runi1";
const TOKEN_ID_BYTES: usize = 8;
const TOKEN_SECRET_BYTES: usize = 32;

/// decoded enrollment token. the service url is authenticated by the secret-bearing token and
/// tells a new agent where to redeem it without a separate configuration step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnrollToken {
    pub token_id: String,
    pub secret: Vec<u8>,
    pub service_url: String,
    pub spki_pin: Option<String>,
    pub cluster_id: Option<Uuid>,
}

impl EnrollToken {
    pub fn generate(service_url: impl Into<String>, spki_pin: Option<String>) -> Self {
        let service_url = service_url.into();
        Self {
            token_id: URL_SAFE_NO_PAD.encode(crate::random_secret(TOKEN_ID_BYTES)),
            secret: crate::random_secret(TOKEN_SECRET_BYTES),
            cluster_id: Some(Uuid::new_v5(
                &Uuid::NAMESPACE_URL,
                service_url.trim_end_matches('/').as_bytes(),
            )),
            service_url,
            spki_pin,
        }
    }

    pub fn encode(&self) -> String {
        let mut parts = vec![
            TOKEN_PREFIX.to_string(),
            self.token_id.clone(),
            URL_SAFE_NO_PAD.encode(&self.secret),
            URL_SAFE_NO_PAD.encode(self.service_url.as_bytes()),
        ];
        if let Some(cluster_id) = self.cluster_id {
            parts.push(
                self.spki_pin
                    .as_deref()
                    .map(|pin| URL_SAFE_NO_PAD.encode(pin.as_bytes()))
                    .unwrap_or_default(),
            );
            let mut binding = Vec::with_capacity(48);
            binding.extend_from_slice(cluster_id.as_bytes());
            binding.extend_from_slice(&self.binding_proof(cluster_id));
            parts.push(URL_SAFE_NO_PAD.encode(binding));
        } else if let Some(pin) = self.spki_pin.as_deref() {
            parts.push(URL_SAFE_NO_PAD.encode(pin.as_bytes()));
        }
        parts.join(".")
    }

    pub fn decode(raw: &str) -> Result<Self, String> {
        let parts = raw.split('.').collect::<Vec<_>>();
        if !matches!(parts.len(), 4..=6) || parts[0] != TOKEN_PREFIX {
            return Err("invalid enrollment token".to_string());
        }
        let token_id_bytes = URL_SAFE_NO_PAD
            .decode(parts[1])
            .map_err(|_| "invalid enrollment token")?;
        let secret = URL_SAFE_NO_PAD
            .decode(parts[2])
            .map_err(|_| "invalid enrollment token")?;
        let service_url = URL_SAFE_NO_PAD
            .decode(parts[3])
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .ok_or_else(|| "invalid enrollment token".to_string())?;
        let spki_pin = parts
            .get(4)
            .filter(|encoded| !encoded.is_empty())
            .map(|encoded| URL_SAFE_NO_PAD.decode(encoded))
            .transpose()
            .ok()
            .flatten()
            .and_then(|bytes| String::from_utf8(bytes).ok());
        let cluster_id = parts.get(5).and_then(|value| {
            let binding = URL_SAFE_NO_PAD.decode(value).ok()?;
            if binding.len() != 48 {
                return None;
            }
            let cluster_id = Uuid::from_slice(&binding[..16]).ok()?;
            let token = Self {
                token_id: parts[1].to_string(),
                secret: secret.clone(),
                service_url: service_url.clone(),
                spki_pin: spki_pin.clone(),
                cluster_id: Some(cluster_id),
            };
            token
                .verify_binding(cluster_id, &binding[16..])
                .then_some(cluster_id)
        });
        if token_id_bytes.len() != TOKEN_ID_BYTES
            || secret.len() != TOKEN_SECRET_BYTES
            || service_url.is_empty()
            || (parts.len() == 5 && spki_pin.is_none())
            || (parts.len() == 6 && cluster_id.is_none())
        {
            return Err("invalid enrollment token".to_string());
        }
        Ok(Self {
            token_id: parts[1].to_string(),
            secret,
            service_url,
            spki_pin,
            cluster_id,
        })
    }

    /// authenticate the exact serialized enrollment request. the secret itself never crosses the
    /// wire; changing even one requested label invalidates the proof.
    pub fn proof(&self, canonical_request: &[u8]) -> Vec<u8> {
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.secret)
            .expect("hmac accepts enrollment secrets of any length");
        mac.update(canonical_request);
        mac.finalize().into_bytes().to_vec()
    }

    pub fn verify_proof(&self, canonical_request: &[u8], proof: &[u8]) -> bool {
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.secret)
            .expect("hmac accepts enrollment secrets of any length");
        mac.update(canonical_request);
        mac.verify_slice(proof).is_ok()
    }

    fn binding_proof(&self, cluster_id: Uuid) -> Vec<u8> {
        self.binding_mac(cluster_id)
            .finalize()
            .into_bytes()
            .to_vec()
    }

    fn binding_mac(&self, cluster_id: Uuid) -> Hmac<Sha256> {
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.secret)
            .expect("hmac accepts enrollment secrets of any length");
        mac.update(b"runinator-enrollment-binding-v1\0");
        mac.update(cluster_id.as_bytes());
        mac.update(self.service_url.as_bytes());
        mac.update(b"\0");
        if let Some(pin) = self.spki_pin.as_deref() {
            mac.update(pin.as_bytes());
        }
        mac
    }

    fn verify_binding(&self, cluster_id: Uuid, proof: &[u8]) -> bool {
        self.binding_mac(cluster_id).verify_slice(proof).is_ok()
    }
}

#[cfg(test)]
#[path = "enroll_tests.rs"]
mod tests;
