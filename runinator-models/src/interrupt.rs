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
/// two families. most sources are **drive-matched**: every park and resume already flows through a
/// typed orchestration event, so a drive always arrives carrying its reason, and a source is a
/// predicate over the node state that drive finds. the rest are **requested** — recorded on the run
/// from outside it and raised by the next drive of the target thread. [`InterruptSource::requested`]
/// is what tells the two apart.
///
/// adding a source is a variant here, an entry in [`InterruptSource::ALL`], and an arm in the
/// reducer's `detect`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterruptSource {
    /// A workflow-owned repeating timer elapsed. Unlike [`Self::Wake`], this timer is independent
    /// of any `wait` node and is declared with its own interval on the handler.
    Timer,
    /// a parked cursor's timer elapsed. bound to a `wait` node's deadline.
    #[default]
    Wake,
    /// the node's own deadline is about to blow while its run is still in flight. raised *before*
    /// the node times out, so a handler can extend the window with `resume restart` or step past it
    /// with `resume next` instead of only reacting to the failure.
    Timeout,
    /// a failed node run is about to be re-dispatched by the retry policy.
    Retry,
    /// a node run settled `Failed` and the thread is about to take its failure route.
    Failure,
    /// an out-of-band park resolution landed: a signal delivered, an approval decided, an input
    /// submitted. a polled park (`gate`) resolves inline and never produces this.
    Resolved,
    /// a child run a `subflow` node is parked on reached a terminal.
    Child,
    /// requested through `POST /workflow_runs/{id}/interrupts`.
    External,
    /// a signal arrived that no node in the run was parked on. without a handler declared for this
    /// source the delivery is rejected exactly as it always was.
    OrphanSignal,
}

impl InterruptSource {
    /// every source, in the precedence the reducer matches them in. the order only decides which
    /// source wins when a single drive satisfies more than one — the more specific reading first.
    pub const ALL: [Self; 9] = [
        Self::External,
        Self::OrphanSignal,
        Self::Timer,
        Self::Wake,
        Self::Timeout,
        Self::Retry,
        Self::Failure,
        Self::Resolved,
        Self::Child,
    ];

    /// the stable wire/author-facing name, matching the serde rename.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Timer => "timer",
            Self::Wake => "wake",
            Self::Timeout => "timeout",
            Self::Retry => "retry",
            Self::Failure => "failure",
            Self::Resolved => "resolved",
            Self::Child => "child",
            Self::External => "external",
            Self::OrphanSignal => "orphan_signal",
        }
    }

    /// true when this source is raised from a [`PendingInterrupt`] recorded on the run rather than
    /// matched against the node state a drive finds.
    pub fn requested(&self) -> bool {
        matches!(self, Self::External | Self::OrphanSignal)
    }
}

impl std::fmt::Display for InterruptSource {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl std::str::FromStr for InterruptSource {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|source| source.as_str() == value)
            .ok_or("unknown interrupt source")
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
    /// every mode, in the order the UI should offer them: the default first, then the decisions
    /// that move the interrupted thread. the catalog and the `resume` node's field read this, so a
    /// new variant reaches both without a second list to keep in step.
    pub const ALL: [Self; 4] = [Self::Resume, Self::Continue, Self::Restart, Self::Fail];

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Resume => "resume",
            Self::Continue => "continue",
            Self::Restart => "restart",
            Self::Fail => "fail",
        }
    }
}

impl std::str::FromStr for InterruptMode {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "resume" => Ok(Self::Resume),
            "continue" => Ok(Self::Continue),
            "restart" => Ok(Self::Restart),
            "fail" => Ok(Self::Fail),
            _ => Err("unknown interrupt mode"),
        }
    }
}

impl std::fmt::Display for InterruptMode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

fn interrupt_enabled() -> bool {
    true
}

fn is_true(value: &bool) -> bool {
    *value
}

/// the key recorded on a cursor once an interrupt has fired at a position, so a plain `resume`
/// does not immediately re-raise the same interrupt against the same node run.
///
/// scoped by `attempt`, not just the node run id: a retry reuses one row across every attempt, so a
/// node-run-id-only key would dedupe every attempt after the first against the retry it fired for.
pub fn handled_key(source: InterruptSource, node_run_id: Uuid, attempt: i64) -> String {
    format!("{source}:{node_run_id}:{attempt}")
}

#[cfg(test)]
#[path = "interrupt_tests.rs"]
mod tests;

mod resume_point;
pub use resume_point::ResumePoint;

mod interrupt_frame;
pub use interrupt_frame::InterruptFrame;

mod pending_interrupt;
pub use pending_interrupt::PendingInterrupt;

mod interrupt_declaration;
pub use interrupt_declaration::InterruptDeclaration;
