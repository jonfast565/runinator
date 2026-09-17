#[allow(unused_imports)]
use super::*;

#[derive(Debug, Serialize, ToSchema)]
pub struct AuthConfigResponseSchema {
    pub enabled: bool,
}
