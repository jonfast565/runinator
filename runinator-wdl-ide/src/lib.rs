// runinator-wdl-ide: the editor surface over the wdl language core. completion and hover answer
// "what can go here" and "what is this" for a cursor in a source buffer — author-time assistance,
// not language semantics. they read a document through `runinator-wdl`'s parser, ast, and
// `analysis` seam, and never influence what a compiled workflow means.
//
// this lives apart from `runinator-wdl` so the language core stays the thing every service links:
// the ws handlers, the lsp, and the tauri client take this crate for the two editor endpoints,
// while ctl, the worker, and the compiler path depend only on the core.

mod completion;
mod hover;

#[cfg(test)]
mod lib_tests;

pub use completion::{
    WdlCompletionItem, WdlCompletionRequest, WdlCompletionResponse, complete_source,
};
pub use hover::{WdlHoverRequest, WdlHoverResponse, hover_source};
