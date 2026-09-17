#[allow(unused_imports)]
use super::*;

#[derive(Serialize, Deserialize)]
pub(super) struct RootFs {
    #[serde(rename = "type")]
    pub(super) kind: String,
    pub(super) diff_ids: Vec<String>,
}
