#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize)]
pub struct DevPackFile {
    pub path: String,
    pub kind: String,
    pub size_bytes: Option<u64>,
    pub modified_at: Option<DateTime<Utc>>,
}
