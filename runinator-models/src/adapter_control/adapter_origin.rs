#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct AdapterOrigin {
    pub adapter_id: Uuid,
    pub revision: i64,
    pub delivery_record_id: Option<Uuid>,
}
