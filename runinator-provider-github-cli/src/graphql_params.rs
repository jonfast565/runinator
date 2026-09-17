#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub(super) struct GraphqlParams {
    pub(super) query: String,
    #[serde(default)]
    pub(super) variables: BTreeMap<String, Value>,
    #[serde(default)]
    pub(super) hostname: Option<String>,
}
