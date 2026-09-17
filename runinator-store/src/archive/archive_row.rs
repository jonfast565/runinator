#[allow(unused_imports)]
use super::*;

#[derive(Clone, Debug)]
pub struct ArchiveRow {
    pub mark_id: Uuid,
    pub table: ArchiveTable,
    pub primary_key: Uuid,
    pub created_at: DateTime<Utc>,
    pub row: Value,
}
