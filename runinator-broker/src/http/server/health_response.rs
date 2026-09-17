#[allow(unused_imports)]
use super::*;

#[derive(Serialize)]
pub(super) struct HealthResponse {
    pub(super) status: &'static str,
}
