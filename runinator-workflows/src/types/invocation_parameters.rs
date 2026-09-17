#[allow(unused_imports)]
use super::*;

/// an `invocation` node's compiled program plus how long one call of it may take.
///
/// the module is held decoded rather than as raw json: parsing it here is what makes an
/// undecodable module a *validation* error, caught when the definition is saved, instead of a
/// runtime failure on the first run that reaches the node.
#[derive(Debug, Clone, PartialEq)]
pub struct InvocationParameters {
    pub module: InvocationModule,
    /// the per-call deadline the node's policy supplies, which a `with { }` postfix may override.
    pub timeout_seconds: Option<i64>,
}
