#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub struct GateQuery {
    #[serde(default)]
    pub workflow_run_id: Option<Uuid>,
    #[serde(default)]
    pub status: Option<String>,
}
