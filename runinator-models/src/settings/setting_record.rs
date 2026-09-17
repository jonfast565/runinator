#[allow(unused_imports)]
use super::*;

/// a stored setting's full persisted form: identity, the value bytes as held at rest (ciphertext
/// when the store encrypts), and the unix-seconds modification time used for import
/// reconciliation. this is a persistence record, not a wire type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingRecord {
    /// Durable logical identity. Scope/name are the human-facing alias and may move without
    /// changing a persisted consumer reference.
    pub id: Uuid,
    pub org_id: Option<Uuid>,
    pub kind: SettingKind,
    pub scope: String,
    pub name: String,
    pub value: Vec<u8>,
    pub updated_at: i64,
}
