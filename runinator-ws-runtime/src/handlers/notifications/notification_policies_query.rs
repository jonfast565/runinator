#[allow(unused_imports)]
use super::*;

#[derive(Deserialize, Default)]
pub struct NotificationPoliciesQuery {
    /// narrow to one workflow's own policies; omit for every policy including the global ones.
    #[serde(default)]
    pub workflow_id: Option<Uuid>,
}
