// runinator-rexrap-ide: the editor surface over the rexrap language core. completion and hover answer
// "what can go here" and "what is this" for a cursor in a source buffer — author-time assistance,
// not language semantics. they read a document through `runinator-rexrap`'s parser, ast, and
// `analysis` seam, and never influence what a compiled workflow means.
//
// this lives apart from `runinator-rexrap` so the language core stays the thing every service links:
// The WS handlers, the LSP, and the Tauri client use this crate for the two editor endpoints,
// while ctl, the worker, and the compiler path depend only on the core.

mod completion;
mod cursor;
mod hover;

#[cfg(test)]
mod lib_tests;

pub use completion::{
    RexRapCompletionItem, RexRapCompletionRequest, RexRapCompletionResponse, complete_source,
};
pub use hover::{RexRapHoverRequest, RexRapHoverResponse, hover_source};
