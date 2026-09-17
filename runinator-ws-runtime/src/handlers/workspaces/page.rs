#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub struct Page {
    #[serde(default = "page_size")]
    pub(super) limit: i64,
    #[serde(default)]
    pub(super) offset: i64,
}
