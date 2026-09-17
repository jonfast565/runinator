#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackfillResponse {
    pub trigger_id: Uuid,
    pub workflow_id: Uuid,
    /// slots inside the range that already had a firing recorded, so they were left alone.
    pub already_fired: i64,
    /// slots that produced a run, or would have on a dry run.
    pub fired: i64,
    /// true when the range held more slots than `limit` allowed.
    pub truncated: bool,
    pub dry_run: bool,
    pub run_ids: Vec<Uuid>,
    pub slots: Vec<DateTime<Utc>>,
}
