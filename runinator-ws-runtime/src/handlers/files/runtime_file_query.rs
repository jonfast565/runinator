#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize, Default)]
pub struct RuntimeFileQuery {
    pub consumer_run_id: Option<Uuid>,
}
