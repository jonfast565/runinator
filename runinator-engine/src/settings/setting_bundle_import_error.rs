#[allow(unused_imports)]
use super::*;

/// A setting bundle failed either validation (safe to report as a bad request) or persistence.
#[derive(Debug)]
pub struct SettingBundleImportError {
    pub bad_request: bool,
    pub message: String,
}
