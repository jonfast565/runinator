#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AuthenticationSettings {
    pub max_refreshes: u64,
}

impl Default for AuthenticationSettings {
    fn default() -> Self {
        Self { max_refreshes: 100 }
    }
}
