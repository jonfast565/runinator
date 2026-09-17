use chrono::{DateTime, Utc};
use serde::{Serialize, de::DeserializeOwned};

use crate::execution_profiles::{ExecutionProfile, ExecutionProfilePutRequest};
use crate::providers::ProviderMetadata;
use crate::settings::SettingKind;
use crate::value::Value;

/// Marker trait for typed import bundles posted to the web service.
///
/// Implementations advertise their HTTP resource path so the API client and
/// importer can be generic over bundle kind.

const fn default_settings_bundle_version() -> u32 {
    1
}

/// Compatibility names retained for downstream callers during the settings-wire transition.
pub type SecretBundle = SettingsBundle;
pub type SecretBundleEntry = SettingBundleEntry;

mod bundle;
pub use bundle::Bundle;

mod provider_bundle;
pub use provider_bundle::ProviderBundle;

mod setting_bundle_entry;
pub use setting_bundle_entry::SettingBundleEntry;

mod execution_profile_bundle_entry;
pub use execution_profile_bundle_entry::ExecutionProfileBundleEntry;

mod execution_profile_import_result;
pub use execution_profile_import_result::ExecutionProfileImportResult;

mod settings_bundle;
pub use settings_bundle::SettingsBundle;

mod pack_import_result;
pub use pack_import_result::PackImportResult;
