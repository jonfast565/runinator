#[allow(unused_imports)]
use super::*;

#[derive(Default, serde::Deserialize)]
pub struct ReplayPlanQuery {
    pub from_step_id: Option<String>,
}
