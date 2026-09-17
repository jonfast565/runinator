#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginResponse {
    pub access_token: String,
    pub refresh_token: String,
    /// access-token lifetime in seconds.
    pub expires_in: i64,
    pub user: User,
    pub assignments: Vec<RoleAssignment>,
    pub effective_actions: Vec<Action>,
}
