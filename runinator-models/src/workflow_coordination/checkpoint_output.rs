#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointOutput {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkpoint_id: Option<Uuid>,
}
