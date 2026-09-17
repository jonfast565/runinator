#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub struct BrokerMessageQuery {
    pub adapter_id: Option<Uuid>,
    #[serde(default)]
    pub workflow_run_id: Option<Uuid>,
    #[serde(default)]
    pub pipeline_run_id: Option<Uuid>,
    #[serde(default)]
    pub channel: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
}
