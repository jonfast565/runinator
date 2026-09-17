use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::value::{Map, Value};
use crate::workflows::{WorkflowNodeKind, WorkflowStatus};

/// debug step granularity: pause before every node, or only at breakpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DebugMode {
    #[default]
    StepAll,
    Breakpoints,
}

fn default_concurrency() -> i64 {
    1
}

mod subflow_parent;
pub use subflow_parent::SubflowParent;

mod event_source_entry;
pub use event_source_entry::EventSourceEntry;

mod control_frame;
pub use control_frame::ControlFrame;

mod debug_frame;
pub use debug_frame::DebugFrame;

mod debug_config;
pub use debug_config::DebugConfig;

mod debug_runtime;
pub use debug_runtime::DebugRuntime;

mod loop_frame;
pub use loop_frame::LoopFrame;

mod map_frame;
pub use map_frame::MapFrame;

mod map_child;
pub use map_child::MapChild;

mod map_child_state;
pub use map_child_state::MapChildState;

mod compensation_frame;
pub use compensation_frame::CompensationFrame;

mod try_frame;
pub use try_frame::TryFrame;
