//! Shared organization-aware lifecycle for config and secret settings.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use runinator_models::{
    bundles::{SettingBundleEntry, SettingsBundle},
    errors::SendableError,
    settings::{SettingBinding, SettingKind, SettingRecord},
    value::Value,
};
use runinator_secrets::secret_cipher::SecretCipher;
use runinator_store::{
    RuntimeStore,
    roles::{DefinitionStore, SettingStore},
};
use uuid::Uuid;

/// Complete input for one config/secret lifecycle operation.
mod setting_operations;
pub use setting_operations::SettingOperations;

mod setting_configuration;
pub use setting_configuration::SettingConfiguration;
