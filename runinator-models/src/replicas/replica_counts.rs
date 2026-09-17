#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicaCounts {
    pub workers: i64,
    pub wakers: i64,
    pub webservices: i64,
    #[serde(default)]
    pub background: i64,
}
