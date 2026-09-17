#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplaySeedReceipt {
    pub node_id: String,
    pub effect_id: Uuid,
    pub attempt: u32,
}
