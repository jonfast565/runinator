// interrupts: suspend one thread of control, run a handler region on a second cursor in the same
// run, then return control to the suspended thread.
//
// an interrupt is a side-channel. the handler shares the run's context and decides how the
// suspended thread proceeds, but it cannot end the run — anything that settles a run travels
// through the interrupted thread's own graph edges. see `InterruptMode` for the four decisions.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::value::Value;
use crate::workflow_state::{LoopFrame, TryFrame};

/// what raised an interrupt.
///
/// every park and resume already flows through a typed orchestration event, so a drive always
/// arrives carrying its reason; a source is a predicate over that drive. adding one is a variant
/// here plus an arm in the reducer's `source_for_drive`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterruptSource {
    /// a parked cursor's timer elapsed. bound to a `wait` node's deadline.
    #[default]
    Wake,
}

impl InterruptSource {
    /// the stable wire/author-facing name, matching the serde rename.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Wake => "wake",
        }
    }

    /// parse an author-facing source name, for the wdl front end and definition metadata.
    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "wake" => Some(Self::Wake),
            _ => None,
        }
    }
}

impl std::fmt::Display for InterruptSource {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// what a handler's `resume` node does to the thread it interrupted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterruptMode {
    /// resume at the same node and let its handler re-read the node run: a terminal action
    /// transitions, a park re-parks, an unstarted node executes.
    #[default]
    Resume,
    /// settle the interrupted node `Succeeded` and take its success edge.
    Continue,
    /// cancel the in-flight node run and re-enter the node fresh, which resets a park's window.
    Restart,
    /// settle the interrupted node `Failed` and take its `on_failure` edge. this does not itself
    /// fail the run — whether the run ends is the main flow's business.
    Fail,
}

impl InterruptMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Resume => "resume",
            Self::Continue => "continue",
            Self::Restart => "restart",
            Self::Fail => "fail",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "resume" => Some(Self::Resume),
            "continue" => Some(Self::Continue),
            "restart" => Some(Self::Restart),
            "fail" => Some(Self::Fail),
            _ => None,
        }
    }
}

impl std::fmt::Display for InterruptMode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// where a suspended cursor goes back to, snapshotted when the interrupt is raised.
///
/// restoring the whole point rather than diffing it is what makes `finish_interrupt` idempotent: a
/// duplicated drive writes the same position and frames it would have written the first time.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ResumePoint {
    #[serde(default)]
    pub node_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_frame: Option<LoopFrame>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub try_frame: Option<TryFrame>,
}

/// marks a cursor as an interrupt handler rather than an ordinary thread of control.
///
/// every field defaults. a frame that silently degraded to `None` would un-suspend a cursor
/// mid-handler, but failing the parse is worse: `WorkflowRunState::from_state` falls back to
/// `unwrap_or_default`, which would discard every cursor in the run. so the frame is made
/// structurally incapable of failing to parse instead.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct InterruptFrame {
    /// the cursor this handler suspended, and will return control to.
    #[serde(default)]
    pub interrupted_cursor: Uuid,
    #[serde(default)]
    pub source: InterruptSource,
    /// what the raising event carried, readable in the region as `interrupt.payload`.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub payload: Value,
    #[serde(default)]
    pub resume: ResumePoint,
    #[serde(default = "Utc::now")]
    pub raised_at: DateTime<Utc>,
}

/// one declared handler: which source it answers, and the node its region starts at.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InterruptDeclaration {
    /// the source this handler answers. stored as a string so an unknown source from a newer
    /// binary is ignored rather than failing the whole definition parse.
    pub on: String,
    /// the region's entry node id.
    pub handler: String,
}

impl InterruptDeclaration {
    /// the parsed source, or `None` when this declaration names a source this binary does not know.
    pub fn source(&self) -> Option<InterruptSource> {
        InterruptSource::from_str(&self.on)
    }
}

/// the key recorded on a cursor once an interrupt has fired at a position, so a plain `resume`
/// does not immediately re-raise the same interrupt against the same node run.
pub fn handled_key(source: InterruptSource, node_run_id: Uuid) -> String {
    format!("{source}:{node_run_id}")
}

#[cfg(test)]
#[path = "interrupt_tests.rs"]
mod tests;
