#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngressEventRecord {
    pub entry: IngressInboxEntry,
    pub duplicate: bool,
}
