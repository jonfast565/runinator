#[allow(unused_imports)]
use super::*;

pub(crate) struct NotificationPayload {
    pub workflow_run_id: Option<Uuid>,
    pub channel: String,
    pub severity: String,
    pub title: String,
    pub body: Option<String>,
    pub target: Option<String>,
    pub metadata: Value,
}
