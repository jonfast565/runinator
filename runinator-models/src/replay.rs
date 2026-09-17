//! Read-only replay safety review and the acknowledgement bound to it.
use crate::{value::Value, workflows::WorkflowDefinition};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplayVerdict {
    Safe,
    Review,
    Blocked,
}

mod replay_seed_receipt;
pub use replay_seed_receipt::ReplaySeedReceipt;

mod replay_action;
pub use replay_action::ReplayAction;

mod replay_plan;
pub use replay_plan::ReplayPlan;

mod replay_options;
pub use replay_options::ReplayOptions;
