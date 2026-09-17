#[allow(unused_imports)]
use super::*;

#[derive(Serialize, ToSchema)]
pub struct HealthResponse {
    pub(super) status: String,
}
