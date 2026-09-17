#[allow(unused_imports)]
use super::*;

#[derive(Deserialize, Default)]
pub struct CalendarQuery {
    #[serde(default)]
    pub scope: CalendarScope,
    #[serde(default)]
    pub org_id: Option<Uuid>,
    #[serde(default)]
    pub horizon_days: Option<i64>,
}
