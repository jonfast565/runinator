//! immutable history for authored workflow definitions.
//!
//! every accepted definition is captured as a `WorkflowRevision` so a change can be seen, attributed,
//! and rolled back. the `workflows` row stays the mutable head; revisions are append-only.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::semver::SemVer;
use crate::types::RuninatorType;
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

/// who is writing a definition, and why. passed into the engine's save path so a revision can be
/// attributed without the engine depending on the web service's auth extraction — the same shape
/// `record_audit` uses for the audit trail.
#[derive(Debug, Clone, Default)]
pub struct RevisionAuthor {
    pub actor_id: Option<Uuid>,
    pub actor_kind: String,
    pub source: RevisionSource,
    pub note: Option<String>,
}

impl RevisionAuthor {
    /// an unattributed write performed by the platform itself (background reconcile, tests).
    pub fn system(source: RevisionSource) -> Self {
        Self {
            actor_id: None,
            actor_kind: "system".to_string(),
            source,
            note: None,
        }
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }
}

/// one immutable capture of a workflow definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRevision {
    pub id: Uuid,
    pub workflow_id: Uuid,
    /// monotonic per workflow, 1-based. `revision` is the stable handle a rollback names.
    pub revision: i64,
    pub version: SemVer,
    pub name: String,
    #[serde(default)]
    pub input_type: RuninatorType,
    #[serde(default)]
    pub definition: WorkflowGraph,
    pub source: RevisionSource,
    #[serde(default)]
    pub actor_id: Option<Uuid>,
    pub actor_kind: String,
    #[serde(default)]
    pub note: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
}

impl WorkflowRevision {
    /// rebuild a savable definition from this revision, carrying the *current* row's identity
    /// (id/namespace/org/enabled) so restoring never re-tenants or re-enables a workflow.
    pub fn to_definition(&self, current: &WorkflowDefinition) -> WorkflowDefinition {
        WorkflowDefinition {
            id: current.id,
            name: self.name.clone(),
            namespace: current.namespace.clone(),
            org_id: current.org_id,
            version: self.version,
            enabled: current.enabled,
            input_type: self.input_type.clone(),
            definition: self.definition.clone(),
            created_at: current.created_at,
            updated_at: None,
        }
    }
}
