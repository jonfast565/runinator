#[allow(unused_imports)]
use super::*;

/// user identity and current platform authority.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserView {
    #[serde(flatten)]
    pub user: User,
    pub platform_role: Option<PlatformRole>,
}
