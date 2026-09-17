#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub struct DiffQuery {
    pub(super) before: i64,
    pub(super) cursor: Option<String>,
}
