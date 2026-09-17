//! immutable history for authored workflow definitions.
//!
//! every accepted definition is captured as a `WorkflowRevision` so a change can be seen, attributed,
//! and rolled back. the `workflows` row stays the mutable head; revisions are append-only.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::pipelines::{Pipeline, PipelineDefaults, PipelineGraph};
use crate::schedules::WorkflowConcurrency;
use crate::semver::SemVer;
use crate::types::RuninatorType;
use crate::value::Value;
use crate::workflows::{WorkflowDefinition, WorkflowGraph};

/// where an accepted definition came from. recorded per revision so a pack reconcile is
/// distinguishable from a hand edit after the fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RevisionSource {
    /// saved from the command center.
    Ui,
    /// applied by a pack import (`runinatorctl workflows apply`).
    Pack,
    /// written through the http API by a client that is not the command center.
    #[default]
    Api,
    /// created by duplicating an existing workflow into a sibling version.
    Duplicate,
    /// written by restoring an earlier revision.
    Rollback,
}

impl RevisionSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            RevisionSource::Ui => "ui",
            RevisionSource::Pack => "pack",
            RevisionSource::Api => "api",
            RevisionSource::Duplicate => "duplicate",
            RevisionSource::Rollback => "rollback",
        }
    }

    /// every source in a stable, UI-facing order.
    pub const ALL: [RevisionSource; 5] = [
        RevisionSource::Ui,
        RevisionSource::Pack,
        RevisionSource::Api,
        RevisionSource::Duplicate,
        RevisionSource::Rollback,
    ];
}

impl TryFrom<&str> for RevisionSource {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "ui" => Ok(RevisionSource::Ui),
            "pack" => Ok(RevisionSource::Pack),
            "api" => Ok(RevisionSource::Api),
            "duplicate" => Ok(RevisionSource::Duplicate),
            "rollback" => Ok(RevisionSource::Rollback),
            other => Err(format!("unknown revision source '{other}'")),
        }
    }
}

impl std::fmt::Display for RevisionSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

mod revision_author;
pub use revision_author::RevisionAuthor;

mod workflow_revision;
pub use workflow_revision::WorkflowRevision;

mod pipeline_revision;
pub use pipeline_revision::PipelineRevision;
