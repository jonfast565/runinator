#[allow(unused_imports)]
use super::*;

#[derive(Debug, Serialize, ToSchema)]
pub struct LoginRequestSchema {
    pub username: String,
    pub password: String,
}
