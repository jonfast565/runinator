//! the unified config/secret store.
//!
//! one of the role traits `DatabaseImpl` composes. bound on this directly when a caller only
//! needs this slice of the store.

use std::future::Future;

use runinator_models::{
    errors::SendableError,
    settings::{SettingKind, SettingRecord},
};

/// Core persistence operations for Runinator.
/// The unified config/secret store.
pub trait SettingStore: Send + Sync + 'static {
    /// List stored settings, including encrypted values, in stable logical order.
    fn list_stored_settings(
        &self,
        org_id: Option<uuid::Uuid>,
    ) -> impl Future<Output = Result<Vec<SettingRecord>, SendableError>> + Send;

    /// Insert or replace a setting's stored value (encrypted at rest) and modification time.
    fn upsert_setting(
        &self,
        org_id: Option<uuid::Uuid>,
        kind: SettingKind,
        scope: String,
        name: String,
        value: Vec<u8>,
        updated_at: i64,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Delete a setting; succeeds even when the entry is absent.
    fn delete_setting(
        &self,
        org_id: Option<uuid::Uuid>,
        kind: SettingKind,
        scope: String,
        name: String,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Move a setting's human-facing alias while preserving its durable UUID and current value.
    fn move_setting(
        &self,
        id: uuid::Uuid,
        org_id: Option<uuid::Uuid>,
        kind: SettingKind,
        scope: String,
        name: String,
    ) -> impl Future<Output = Result<Option<SettingRecord>, SendableError>> + Send;
}
