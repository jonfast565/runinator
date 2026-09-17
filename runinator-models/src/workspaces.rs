use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::value::Value;

/// Effect-output stream reserved for bounded durable workspace lifecycle summaries.
pub const WORKSPACE_TIMELINE_STREAM: &str = "runinator.workspace";

/// Reserved worker label used to route filesystem-bound effects to a stable machine identity.
/// A worker runtime may mint a new replica id after restart; its instance id remains stable.
pub const WORKSPACE_INSTANCE_LABEL: &str = "runinator.instance";

/// Worker capability for restoring portable snapshots.
pub const PORTABLE_WORKSPACE_LABEL: &str = "runinator.workspace.portable";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceStatus {
    Allocating,
    Active,
    Finalizing,
    Released,
    Abandoned,
}

impl WorkspaceStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allocating => "allocating",
            Self::Active => "active",
            Self::Finalizing => "finalizing",
            Self::Released => "released",
            Self::Abandoned => "abandoned",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Released | Self::Abandoned)
    }
}

impl TryFrom<&str> for WorkspaceStatus {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "allocating" => Ok(Self::Allocating),
            "active" => Ok(Self::Active),
            "finalizing" => Ok(Self::Finalizing),
            "released" => Ok(Self::Released),
            "abandoned" => Ok(Self::Abandoned),
            other => Err(format!("unknown workspace status {other}")),
        }
    }
}

// durable contents have their own identity and version sequence, independent of local leases.
#[path = "workspace_contents.rs"]
mod contents;
pub use contents::*;
#[path = "workspace_limits.rs"]
mod limits;
pub use limits::*;

mod workspace_phase_event;
pub use workspace_phase_event::WorkspacePhaseEvent;

mod workspace_affinity;
pub use workspace_affinity::WorkspaceAffinity;

mod workspace_lease;
pub use workspace_lease::WorkspaceLease;

mod new_workspace_lease;
pub use new_workspace_lease::NewWorkspaceLease;
