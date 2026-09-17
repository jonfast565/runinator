// a run's position in its workflow graph, and the frames scoped to that position.
//
// a run is a track and a cursor is a place on it. the run carries its cursors in
// `WorkflowExecutionState`; `workflow_runs.active_node_id` mirrors the primary one so the wire and UI
// contract is unchanged.
//
// frames that belong to one thread of control (a loop iteration, a try phase) live on the cursor
// rather than on the run, because two cursors running concurrently would otherwise share — and
// corrupt — a single frame.

use std::collections::BTreeSet;
use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::interrupt::InterruptFrame;
use crate::value::Value;
use crate::workflow_state::{DebugRuntime, LoopFrame, TryFrame};
use crate::workflows::WorkflowRun;

fn is_zero(value: &i64) -> bool {
    *value == 0
}

#[cfg(test)]
#[path = "cursor_tests.rs"]
mod cursor_tests;

mod speculative_frame;
pub use speculative_frame::SpeculativeFrame;

mod run_cursor;
pub use run_cursor::RunCursor;
