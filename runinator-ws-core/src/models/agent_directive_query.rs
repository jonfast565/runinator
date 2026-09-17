#[allow(unused_imports)]
use super::*;

#[derive(Debug, Default, Deserialize)]
pub struct AgentDirectiveQuery {
    pub limit: Option<i64>,
}
