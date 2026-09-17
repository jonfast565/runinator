#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Modifiers {
    pub timeout_seconds: Option<i64>,
    pub retry: Option<RetryConfig>,
    pub tags: Vec<String>,
    pub mcp: bool,
    pub reentry: Option<Reentry>,
    /// `.runner("<type>")`: require a worker carrying the `runner=<type>` label to execute this node.
    pub runner: Option<String>,
    /// `@profile("name")`: select a centrally managed execution profile for this action.
    pub profile: Option<String>,
    /// `@workspace(<expr>)`: route this call to the stable worker instance held in a
    /// `WorkspaceAffinity` value.
    pub workspace_affinity: Option<Expr>,
    /// `.idempotent(key: <expr>)`: name this action's external effect. the reducer resolves the
    /// expression per dispatch and the worker reserves the result, replaying a recorded outcome
    /// instead of invoking the provider twice for the same key.
    pub idempotency_key: Option<Expr>,
}
