#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewOrchestrationEvent {
    pub event_id: Uuid,
    pub workflow_run_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_node_run_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    /// the thread of control this wake belongs to. stamped onto the ready-node row so a run with
    /// fan-out can wake one branch without disturbing its siblings. `None` for a wake that predates
    /// cursor-keyed arming, which resolves by node id as before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor_id: Option<Uuid>,
    pub event_type: String,
    #[serde(default)]
    pub payload: Value,
    pub created_at: DateTime<Utc>,
}

impl NewOrchestrationEvent {
    pub fn new(
        workflow_run_id: Uuid,
        node_id: Option<String>,
        event_type: impl Into<String>,
        payload: Value,
    ) -> Self {
        Self {
            event_id: Uuid::now_v7(),
            workflow_run_id,
            workflow_node_run_id: None,
            node_id,
            cursor_id: None,
            event_type: event_type.into(),
            payload,
            created_at: Utc::now(),
        }
    }

    /// address this wake to one cursor.
    pub fn for_cursor(mut self, cursor_id: Uuid) -> Self {
        self.cursor_id = Some(cursor_id);
        self
    }
}
