#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub struct WorkerQuery {
    pub(super) replica_id: Uuid,
}
