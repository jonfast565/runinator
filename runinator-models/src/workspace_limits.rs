//! Deployment policy captured when a workspace checkout or import is admitted.

use crate::errors::{SendableError, WORKSPACE_INVALID, WORKSPACE_LIMIT};
use serde::{Deserialize, Serialize};

#[path = "workspace_limits/workspace_limits.rs"]
mod workspace_limits;
pub use workspace_limits::WorkspaceLimits;

#[path = "workspace_limits/workspace_usage.rs"]
mod workspace_usage;
pub use workspace_usage::WorkspaceUsage;
