#[allow(unused_imports)]
use super::*;

#[derive(Clone, Debug)]
pub struct ArchiveMark {
    pub id: Uuid,
    pub table: ArchiveTable,
    pub primary_key: Uuid,
    pub created_at: DateTime<Utc>,
    pub eligible_before: DateTime<Utc>,
    pub archive_day: String,
}
