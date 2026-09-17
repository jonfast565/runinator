#[allow(unused_imports)]
use super::*;

#[derive(Serialize)]
pub(super) struct GitResult {
    pub(super) stdout: String,
    pub(super) action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) workspace: Option<String>,
}
