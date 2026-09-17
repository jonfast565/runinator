#[allow(unused_imports)]
use super::*;

#[derive(Debug, Serialize, ToSchema)]
pub struct LoginResponseSchema {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i64,
    pub user: UserSchema,
}
