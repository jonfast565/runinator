#[allow(unused_imports)]
use super::*;

/// Result of permanently invalidating one enrolled machine identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentMachineInvalidation {
    pub revoked_credentials: usize,
    pub kicked_replicas: u64,
}
