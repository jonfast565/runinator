//! Deployment policy captured when a workspace checkout or import is admitted.

use crate::errors::{SendableError, WORKSPACE_INVALID, WORKSPACE_LIMIT};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceLimits {
    pub max_bytes: u64,
    pub max_entries: u64,
    pub max_results_bytes: u64,
}

impl Default for WorkspaceLimits {
    fn default() -> Self {
        Self {
            max_bytes: 128 * 1024 * 1024 * 1024,
            max_entries: 100_000,
            max_results_bytes: 16 * 1024 * 1024,
        }
    }
}

impl WorkspaceLimits {
    pub fn validate(self) -> Result<Self, SendableError> {
        if self.max_bytes == 0 || self.max_entries == 0 || self.max_results_bytes == 0 {
            return Err(WORKSPACE_INVALID.error("workspace limits must be positive"));
        }
        Ok(self)
    }

    pub fn check(self, usage: WorkspaceUsage) -> Result<(), SendableError> {
        self.validate()?;
        for (name, actual, limit) in [
            ("logical bytes", usage.logical_bytes, self.max_bytes),
            ("entries", usage.entries, self.max_entries),
            (
                "named result bytes",
                usage.results_bytes,
                self.max_results_bytes,
            ),
        ] {
            if actual > limit {
                return Err(WORKSPACE_LIMIT.error(format!("{name}: {actual} exceeds {limit}")));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceUsage {
    /// Distinct inode lengths including sparse holes, plus canonical named result bytes.
    pub logical_bytes: u64,
    /// Directory entries, including aliases and symbolic links, excluding the root.
    pub entries: u64,
    pub results_bytes: u64,
}
