// shared debugger domain: the break decision, provenance markers, and the `Debuggable`
// abstraction. this is pure logic over the typed debug frame so both the scheduler (which decides
// when to pause) and the web service (which applies debug commands) share one implementation
// instead of each re-deriving the rules against raw json.

use crate::cursor::RunCursor;
use crate::value::Value;
use crate::workflow_state::{DebugConfig, DebugFrame, DebugMode, DebugRuntime};
use crate::workflows::WorkflowRun;

/// node-run provenance marker for a node skipped by the debugger.
pub const DEBUG_SKIPPED: &str = "debug_skipped";
/// node-run provenance marker for a prior attempt superseded by a debug re-run.
pub const DEBUG_SUPERSEDED: &str = "debug_superseded";
/// node-run provenance marker for the fresh attempt created by a debug re-run.
pub const DEBUG_RERUN: &str = "debug_rerun";
/// node-run provenance marker for a shadowed node whose output was replayed from a real run.
pub const DEBUG_SHADOW_REPLAY: &str = "debug_shadow_replay";
/// node-run provenance marker for a shadowed node with no recorded output to replay.
pub const DEBUG_SHADOW_STUB: &str = "debug_shadow_stub";
/// node-run provenance marker for any node run produced by a speculative cursor.
pub const DEBUG_SPECULATIVE: &str = "debug_speculative";

/// pure break decision: should a target paused at `key` halt before executing it.
///
/// `step_all` always breaks; `breakpoints` breaks only at a configured breakpoint or the one-shot
/// cursor. this intentionally does not consider `enabled`/`step_requested` — those are flow gates
/// owned by the caller, not part of the breakpoint rule.
pub fn should_break_at(config: &DebugConfig, runtime: &DebugRuntime, key: &str) -> bool {
    match config.mode.unwrap_or_default() {
        DebugMode::StepAll => true,
        DebugMode::Breakpoints => {
            config.breakpoints.iter().any(|id| id == key)
                || runtime.one_shot_breakpoint.as_deref() == Some(key)
        }
    }
}

/// a unit of execution that can be paused, inspected, and advanced under debugger control.
///
/// implement this for each new debuggable target (workflow runs today; standalone task runs,
/// subflows, or provider calls later). the default [`Debuggable::should_break_at`] gives every
/// target step/breakpoint/one-shot semantics for free; targets only describe how to read their
/// debug frame and how to key a cursor.
pub trait Debuggable {
    /// identifier of the step the cursor is about to execute (e.g. a workflow node).
    type Cursor;

    /// parsed debug frame for this target, if it carries one.
    fn debug_frame(&self) -> Option<DebugFrame>;

    /// stable string key for `cursor`, matched against breakpoints and the one-shot cursor.
    fn cursor_key(&self, cursor: &Self::Cursor) -> String;

    /// the runtime state governing `cursor`. defaults to the target's run-scoped runtime, which is
    /// the right answer for any target whose cursors do not carry one of their own.
    fn cursor_runtime(&self, _cursor: &Self::Cursor, frame: &DebugFrame) -> DebugRuntime {
        frame.runtime.clone()
    }

    /// whether execution must halt before `cursor` runs, per this target's debug frame.
    fn should_break_at(&self, cursor: &Self::Cursor) -> bool {
        match self.debug_frame() {
            Some(frame) => {
                let runtime = self.cursor_runtime(cursor, &frame);
                should_break_at(&frame.config, &runtime, &self.cursor_key(cursor))
            }
            None => false,
        }
    }
}

/// parse the `debug` object out of a run/node `state` blob, if present.
pub fn parse_frame(state: &Value) -> Option<DebugFrame> {
    let debug = state.get("debug")?;
    serde_json::from_value(debug.clone().into()).ok()
}

impl Debuggable for WorkflowRun {
    type Cursor = RunCursor;

    fn debug_frame(&self) -> Option<DebugFrame> {
        parse_frame(&self.state)
    }

    fn cursor_key(&self, cursor: &Self::Cursor) -> String {
        cursor.node_id().to_string()
    }

    /// each thread of control carries its own runtime, so two branches can be paused at different
    /// nodes. a cursor without one predates per-cursor debug state and reads the run's frame.
    fn cursor_runtime(&self, cursor: &Self::Cursor, frame: &DebugFrame) -> DebugRuntime {
        cursor
            .debug
            .clone()
            .unwrap_or_else(|| frame.runtime.clone())
    }
}
