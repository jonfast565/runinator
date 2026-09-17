#[allow(unused_imports)]
use super::*;

#[derive(Debug, Serialize, ToSchema)]
pub struct RefreshRequestSchema {
    pub refresh_token: String,
}
