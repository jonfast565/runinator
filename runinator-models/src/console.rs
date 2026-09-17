//! the rexrap console: a notebook of cells evaluated against a shared, persisted scope.
//!
//! the console is not a second language or a second runtime. a cell is a fragment of the same REXRAP a
//! workflow is written in, and it is answered one of two ways: a pure fragment is evaluated in
//! process, and anything else becomes a scratch workflow run. that split is the whole design — see
//! `runinator-console` for the decision and `repository/console.rs` for what it does with it.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::validation::{
    LONG_TEXT_MAX, SHORT_TEXT_MAX, Validate, ValidationError, optional_text, required_text,
};

use crate::value::Value;

/// what the classifier decided a cell was.
///
/// persisted rather than re-derived so a reader can see why a cell did or did not start a run
/// without re-classifying its source — which, for an edited cell, would answer about the new text
/// rather than the run that is on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsoleCellKind {
    Expression,
    Do,
    /// A function-only cell that updates the session library without binding a value.
    Library,
    Workflow,
}

impl ConsoleCellKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Expression => "expression",
            Self::Do => "do",
            Self::Library => "library",
            Self::Workflow => "workflow",
        }
    }
}

impl TryFrom<&str> for ConsoleCellKind {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "expression" => Ok(Self::Expression),
            "do" => Ok(Self::Do),
            "library" => Ok(Self::Library),
            "workflow" => Ok(Self::Workflow),
            other => Err(format!("Unknown console cell kind '{other}'")),
        }
    }
}

/// where one cell is in its lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsoleCellStatus {
    /// written but never run.
    Idle,
    /// a scratch workflow run is in flight. a pure cell never sits here — it settles in one request.
    Running,
    Succeeded,
    Failed,
}

impl ConsoleCellStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed)
    }
}

impl TryFrom<&str> for ConsoleCellStatus {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "idle" => Ok(Self::Idle),
            "running" => Ok(Self::Running),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            other => Err(format!("Unknown console cell status '{other}'")),
        }
    }
}

/// the reserved workflow-name prefix a console scratch workflow carries.
pub const CONSOLE_WORKFLOW_PREFIX: &str = "console.";

/// the `metadata.managed_by` value a console scratch workflow carries, so it is filtered out of the
/// workflow list the same way a function adapter is.
pub const CONSOLE_MANAGED_BY: &str = "console";

mod console_session;
pub use console_session::ConsoleSession;

mod console_cell;
pub use console_cell::ConsoleCell;

mod console_binding;
pub use console_binding::ConsoleBinding;

mod console_function;
pub use console_function::ConsoleFunction;

mod new_console_function;
pub use new_console_function::NewConsoleFunction;

mod console_session_detail;
pub use console_session_detail::ConsoleSessionDetail;

mod new_console_cell;
pub use new_console_cell::NewConsoleCell;
