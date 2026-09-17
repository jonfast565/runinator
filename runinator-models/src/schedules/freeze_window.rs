#[allow(unused_imports)]
use super::*;

/// a scheduled suspension of trigger firing. a window with no `workflow_id` freezes every workflow
/// in its org; one with no `org_id` freezes the whole platform.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FreezeWindow {
    pub id: Uuid,
    #[serde(default)]
    pub org_id: Option<Uuid>,
    #[serde(default)]
    pub workflow_id: Option<Uuid>,
    pub name: String,
    #[serde(default)]
    pub reason: Option<String>,
    pub starts_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
    /// Recurring definition. Absent rows are legacy one-shot windows represented by the concrete
    /// `starts_at`/`ends_at` pair above.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<ScheduleSpec>,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
