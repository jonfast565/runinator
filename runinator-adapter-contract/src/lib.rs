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
