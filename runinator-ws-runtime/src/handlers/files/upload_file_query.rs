#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub struct UploadFileQuery {
    pub path: String,
    #[serde(default)]
    pub mime_type: Option<String>,
}
