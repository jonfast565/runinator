#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize)]
pub struct DevPackTextFile {
    pub path: String,
    pub content: String,
    pub modified_at: Option<DateTime<Utc>>,
}
