#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractDependent {
    pub kind: String,
    pub id: Uuid,
    pub name: String,
    /// Pinned consumers remain on their immutable revision.
    pub pinned: bool,
}
