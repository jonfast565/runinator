#[allow(unused_imports)]
use super::*;

/// Read-only process/bootstrap configuration shown beside persisted operating policy. Sensitive
/// values are represented only by their configuration state; changing these requires restarting
/// the process that owns them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuntimeSettingDefinition {
    pub key: String,
    pub section: String,
    pub label: String,
    pub description: String,
    pub value: String,
    pub source: String,
    pub restart_required: bool,
    pub sensitive: bool,
}
