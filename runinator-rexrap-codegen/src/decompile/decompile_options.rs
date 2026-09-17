#[allow(unused_imports)]
use super::*;

/// options controlling how a definition is rendered back to rexrap.
#[derive(Debug, Clone, Default)]
pub struct DecompileOptions {
    /// emit the canonical fully-explicit form: a `start ->` line, an id and happy-path arrow on
    /// every node, and every defaulted value (timeout/retry/limit/concurrency/approval type).
    pub explicit: bool,
}
