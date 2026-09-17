#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub struct ResultQuery {
    pub(super) name: String,
    #[serde(default)]
    pub(super) preview: bool,
}
