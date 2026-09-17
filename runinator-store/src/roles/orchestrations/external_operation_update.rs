#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone)]
pub struct ExternalOperationUpdate {
    pub status: ExternalOperationStatus,
    pub attempt: i64,
    pub ambiguous: bool,
    pub provenance: Value,
    pub receipt: Value,
}
