#[allow(unused_imports)]
use super::*;

#[derive(Serialize, ToSchema)]
pub struct ReadinessResponse {
    pub(super) status: String,
    pub(super) database: String,
    pub(super) broker_effect_channels: bool,
    pub(super) counters: stability::StabilityCounters,
}
