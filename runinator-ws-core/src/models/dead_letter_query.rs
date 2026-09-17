#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub struct DeadLetterQuery {
    #[serde(default)]
    pub channel: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
}
