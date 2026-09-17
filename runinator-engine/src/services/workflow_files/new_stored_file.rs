#[allow(unused_imports)]
use super::*;

pub(super) struct NewStoredFile {
    pub(super) scope: FileScope,
    pub(super) org_id: Option<Uuid>,
    pub(super) owner_id: Option<Uuid>,
    pub(super) path: String,
    pub(super) mime_type: String,
    pub(super) bytes: Vec<u8>,
    pub(super) revision: i64,
}
