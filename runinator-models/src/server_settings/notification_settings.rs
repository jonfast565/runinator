#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NotificationSettings {
    pub scan_interval_seconds: u64,
    pub scan_limit: u64,
    pub secret_expiry_warning_seconds: u64,
    pub delivery_timeout_seconds: u64,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            scan_interval_seconds: 60,
            scan_limit: 500,
            secret_expiry_warning_seconds: 30 * 24 * 60 * 60,
            delivery_timeout_seconds: 30,
        }
    }
}
