#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub struct PipelineRevisionListQuery {
    pub(super) limit: Option<i64>,
}
