//! the rexrap console: a notebook of cells evaluated against a shared, persisted scope.
//!
//! the console is not a second language or a second runtime. a cell is a fragment of the same REXRAP a
//! workflow is written in, and it is answered one of two ways: a pure fragment is evaluated in
//! process, and anything else becomes a scratch workflow run. that split is the whole design — see
//! `runinator-console` for the decision and `repository/console.rs` for what it does with it.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::value::Value;

/// one notebook.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConsoleSession {
    pub id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub org_id: Option<Uuid>,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

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
    Workflow,
}

impl ConsoleCellKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Expression => "expression",
            Self::Do => "do",
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

/// one cell.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConsoleCell {
    pub id: Uuid,
    pub session_id: Uuid,
    /// ordering within the session.
    pub position: i64,
    /// the name this cell's result binds to, if the author gave one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<ConsoleCellKind>,
    pub status: ConsoleCellStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// set only for a cell that became a scratch workflow.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_run_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// one name in a session's scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConsoleBinding {
    pub id: Uuid,
    pub session_id: Uuid,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cell_id: Option<Uuid>,
    pub value: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// a session with everything under it, as the api and ui read it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConsoleSessionDetail {
    #[serde(flatten)]
    pub session: ConsoleSession,
    #[serde(default)]
    pub cells: Vec<ConsoleCell>,
    #[serde(default)]
    pub bindings: Vec<ConsoleBinding>,
}

/// what a caller sends to create or replace a cell.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewConsoleCell {
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// append when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<i64>,
}

/// the reserved workflow-name prefix a console scratch workflow carries.
pub const CONSOLE_WORKFLOW_PREFIX: &str = "console.";

/// the `metadata.managed_by` value a console scratch workflow carries, so it is filtered out of the
/// workflow list the same way a function adapter is.
pub const CONSOLE_MANAGED_BY: &str = "console";
