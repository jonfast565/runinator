use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::artifacts::ArtifactRef;

/// classifies a stored setting: a redacted, late-resolved `Secret` or a
/// non-sensitive, eagerly-resolved `Config` value.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum SettingKind {
    #[default]
    Secret,
    Config,
}

impl SettingKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            SettingKind::Secret => "secret",
            SettingKind::Config => "config",
        }
    }

    /// parse a stored kind tag, defaulting to `Secret` for any unrecognized value.
    pub fn from_str_lossy(raw: &str) -> Self {
        match raw {
            "config" => SettingKind::Config,
            _ => SettingKind::Secret,
        }
    }
}

mod setting_summary;
pub use setting_summary::SettingSummary;

mod setting_record;
pub use setting_record::SettingRecord;

mod setting_binding;
pub use setting_binding::SettingBinding;
