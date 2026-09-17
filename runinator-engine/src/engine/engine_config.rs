#[allow(unused_imports)]
use super::*;

/// Runtime limits for one durable engine instance.
///
/// The ingress limit bounds continuation drives, control commands, and agent directive results that may
/// be processed concurrently. Durable ready-node claims and run-state compare-and-swap writes
/// remain the authority for conflicting work.
#[derive(Debug, Clone, Copy)]
pub struct EngineConfig {
    pub max_concurrent_ingress: usize,
    pub workspace_limits: runinator_models::workspaces::WorkspaceLimits,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            max_concurrent_ingress: 16,
            workspace_limits: Default::default(),
        }
    }
}

impl EngineConfig {
    pub fn normalized(self) -> Self {
        Self {
            max_concurrent_ingress: self.max_concurrent_ingress.max(1),
            workspace_limits: self.workspace_limits,
        }
    }
}
