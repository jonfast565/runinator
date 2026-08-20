//! the unified config/secret store.
//!
//! one of the role traits `DatabaseImpl` composes. bound on this directly when a caller only
//! needs this slice of the store.

use std::future::Future;

use runinator_models::{errors::SendableError, settings::SettingKind};

// re-exported here so callers that reach for the contract at its historical path
// (`runinator_database::interfaces::*`) can import both halves from one place.
pub use crate::runtime_store::RuntimeStore;

/// Core persistence operations for Runinator.
/// The unified config/secret store.
pub trait SettingStore: Send + Sync + 'static {
    /// Insert or replace a setting's stored value (encrypted at rest) and modification time.
    fn upsert_setting(
        &self,
        kind: SettingKind,
        scope: String,
        name: String,
        value: Vec<u8>,
        updated_at: i64,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Delete a setting; succeeds even when the entry is absent.
    fn delete_setting(
        &self,
        kind: SettingKind,
        scope: String,
        name: String,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;
}
