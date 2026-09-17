#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub struct AutomationRecordQuery {
    pub workflow_run_id: Option<Uuid>,
    pub external_item_id: Option<Uuid>,
}
