#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub struct SettingMoveRequest {
    pub kind: SettingKind,
    pub scope: String,
    pub name: String,
}

impl Validate for SettingMoveRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        identifier("scope", &self.scope)?;
        identifier("name", &self.name)
    }
}
