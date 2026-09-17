#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterializedExecutionProfile {
    pub profile_id: Uuid,
    pub revision: i64,
    pub root: String,
    pub home: Option<String>,
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
}
