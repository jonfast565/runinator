#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub struct RevisionListQuery {
    pub(super) limit: Option<i64>,
}
