//! deciding what a console cell *is*, and preparing it to run.
//!
//! a notebook cell is a fragment of REXRAP, and the whole console rests on one question: can this be
//! answered in process, or does it need a workflow run? getting that wrong in either direction is
//! bad in a different way — evaluating something effectful in the web service would run a provider
//! action inside an http handler, and starting a run for `1 + 2` would make an arithmetic cell take
//! a second and leave a row in the run history.
//!
//! so classification is deliberately conservative: a cell is treated as pure **only** when the
//! parser says it is a single expression and every call in it resolves to a pure intrinsic.
//! anything else — an action call, a control-flow statement, several statements — becomes a scratch
//! workflow and goes through the ordinary reducer path, which is where effects belong.
//!
//! this crate is the decision only. it holds no database, no http, and no evaluator of its own: the
//! pure route hands off to `runinator-rexrap`'s fragment evaluator and the effectful route hands off to
//! the compiler, both of which already exist and are already tested.

mod classify;
mod errors;
mod session;

pub use classify::{
    CellKind, Classification, classify, classify_with_functions, workflow_source,
    workflow_source_with_functions,
};
pub use errors::{ConsoleError, DICTIONARY, Result};
pub use session::{
    CELL_SCOPE, CONTEXT_ROOT, ConsoleContext, cell_binding_name, scratch_workflow_name,
};
