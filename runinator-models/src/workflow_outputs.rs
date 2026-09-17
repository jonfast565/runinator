use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::types::RuninatorType;
use crate::value::Value;

mod loop_output;
pub use loop_output::LoopOutput;

mod parallel_output;
pub use parallel_output::ParallelOutput;

mod map_output;
pub use map_output::MapOutput;

mod race_output;
pub use race_output::RaceOutput;

mod switch_output;
pub use switch_output::SwitchOutput;

mod config_summary;
pub use config_summary::ConfigSummary;

mod join_output;
pub use join_output::JoinOutput;

mod subflow_outcome;
pub use subflow_outcome::SubflowOutcome;

mod task_status_output;
pub use task_status_output::TaskStatusOutput;

mod skipped_output;
pub use skipped_output::SkippedOutput;

mod workflow_context_header;
pub use workflow_context_header::WorkflowContextHeader;

mod action_idempotency_record;
pub use action_idempotency_record::ActionIdempotencyRecord;

mod approval_record;
pub use approval_record::ApprovalRecord;
