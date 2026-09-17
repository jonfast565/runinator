#[allow(unused_imports)]
use super::*;

/// A console-only top-level module: zero or more function declarations and an optional bare
/// runtime `do { ... }` block.  This is intentionally not accepted by [`parse_document`], whose
/// contract remains that an authored document contains a workflow.
#[derive(Debug, Clone, PartialEq)]
pub struct ConsoleModule {
    pub language_header: bool,
    pub functions: Vec<FunctionDef>,
    /// The byte span of the bare runtime block, when this is an executable module rather than a
    /// function-only library cell.
    pub run_block_span: Option<Span>,
}
