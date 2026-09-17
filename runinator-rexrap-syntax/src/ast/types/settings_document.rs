#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SettingsDocument {
    pub settings: Vec<SecretDecl>,
    pub execution_profiles: Vec<ProfileDecl>,
}
