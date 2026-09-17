#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub struct DirectoryQuery {
    #[serde(default)]
    pub(super) path: String,
    pub(super) cursor: Option<String>,
    pub(super) limit: Option<usize>,
}
