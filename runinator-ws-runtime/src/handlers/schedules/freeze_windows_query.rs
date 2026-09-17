#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Copy, Deserialize, Default)]
pub struct FreezeWindowsQuery {
    /// narrow to one org's windows; the platform-wide ones are always included, since those are
    /// what actually freeze that org's schedules.
    #[serde(default)]
    pub org_id: Option<Uuid>,
    /// list only the windows in effect right now.
    #[serde(default)]
    pub active: Option<bool>,
}
