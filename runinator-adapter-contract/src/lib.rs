//! Versioned, JSON-only boundary between the adapter host and adapter plugins.
//!
//! The host invokes these symbols only in a disposable child process. Paths point at bounded JSON
//! files so neither Rust layout nor allocator ownership crosses the dynamic-library boundary.

use std::collections::BTreeMap;

use runinator_models::orchestration::{AdapterKindMetadata, NormalizedAdapterEvent};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const ADAPTER_ABI_VERSION: u32 = 1;
pub const MARKER_SYMBOL: &[u8] = b"runinator_adapter_abi_version\0";
pub const NAME_SYMBOL: &[u8] = b"runinator_adapter_name\0";
pub const METADATA_SYMBOL: &[u8] = b"runinator_adapter_metadata\0";
pub const HANDLE_SYMBOL: &[u8] = b"runinator_adapter_handle\0";

/// Verify a bearer credential without leaking a length-dependent early mismatch.
pub fn verify_bearer(expected: &str, authorization: &str) -> bool {
    authorization
        .strip_prefix("Bearer ")
        .is_some_and(|supplied| {
            constant_time_eq::constant_time_eq(supplied.as_bytes(), expected.as_bytes())
        })
}

/// Verify a conventional `sha256=<hex>` (or bare hex) HMAC signature.
pub fn verify_hmac_sha256(secret: &str, body: &[u8], supplied: &str) -> bool {
    use hmac::{Hmac, Mac};
    let supplied = supplied.strip_prefix("sha256=").unwrap_or(supplied);
    if !supplied.len().is_multiple_of(2) {
        return false;
    }
    let Ok(expected) = (0..supplied.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&supplied[index..index + 2], 16))
        .collect::<Result<Vec<_>, _>>()
    else {
        return false;
    };
    let Ok(mut mac) = Hmac::<sha2::Sha256>::new_from_slice(secret.as_bytes()) else {
        return false;
    };
    mac.update(body);
    mac.verify_slice(&expected).is_ok()
}

pub type MarkerFn = unsafe extern "C" fn() -> u32;
pub type NameFn = unsafe extern "C" fn() -> *const std::ffi::c_char;
pub type FileOperationFn =
    unsafe extern "C" fn(*const std::ffi::c_char, *const std::ffi::c_char) -> i32;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterRequest {
    #[serde(default)]
    pub method: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    /// RFC 4648 base64 request bytes.
    pub body_base64: String,
    #[serde(default)]
    pub configuration: Value,
    #[serde(default)]
    pub secrets: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterResponse {
    pub verified: bool,
    #[serde(default)]
    pub events: Vec<NormalizedAdapterEvent>,
    #[serde(default)]
    pub errors: Vec<String>,
}

impl AdapterResponse {
    pub fn rejected(error: impl Into<String>) -> Self {
        Self {
            verified: false,
            events: Vec::new(),
            errors: vec![error.into()],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterMetadataEnvelope {
    pub abi_version: u32,
    pub metadata: AdapterKindMetadata,
}
