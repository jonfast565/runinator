#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ServerSettingDefinition {
    pub key: &'static str,
    pub section: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub unit: &'static str,
    pub kind: ServerSettingKind,
    pub default: u64,
    pub minimum: u64,
    pub maximum: u64,
    pub usual_minimum: u64,
    pub usual_maximum: u64,
}
