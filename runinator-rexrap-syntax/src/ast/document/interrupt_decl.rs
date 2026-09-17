#[allow(unused_imports)]
use super::*;

/// a header `interrupt on <source> [every <duration>] { ... }` handler. the block is a region: its
/// first statement is where the interrupt enters, and every path out of it ends at a `resume`.
#[derive(Debug, Clone, PartialEq)]
pub struct InterruptDecl {
    /// the author-facing source name (`wake`). kept as a string so a source this binary does not
    /// know is a lowering-time diagnostic rather than a parse failure.
    pub source: String,
    /// Required only for `interrupt on timer`: the repeating cadence measured from run start.
    pub interval_seconds: Option<i64>,
    pub enabled: bool,
    pub body: Block,
}
