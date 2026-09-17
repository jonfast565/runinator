#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct ExportedCredentials {
    pub(super) access_key_id: String,
    pub(super) secret_access_key: String,
    #[serde(default)]
    pub(super) session_token: Option<String>,
}
