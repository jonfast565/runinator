#[allow(unused_imports)]
use super::*;

pub struct SettingConfiguration {
    pub org_id: Option<Uuid>,
    pub kind: SettingKind,
    pub scope: String,
    pub name: String,
    pub value: Value,
    pub schema: Option<Value>,
    pub expires_at: Option<DateTime<Utc>>,
}
