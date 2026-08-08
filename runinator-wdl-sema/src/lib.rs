// runinator-wdl-sema: the author-time analysis layer, sitting between the parser and lowering.
// it takes an ast and answers what the program *means* without producing any runtime artifact:
// namespace resolution, alias desugaring, the callable registry (intrinsics + user `fn`s),
// purity classification, named-type resolution, and the four semantic passes that emit
// `Diagnostic`s.
//
// the compile options live here too, because this is the lowest crate that reads them: `sema`
// needs the type policy and the pack's workflow signatures, and `runinator-wdl-codegen` needs
// the rest.

pub mod desugar;
pub mod namespace;
pub mod options;
pub mod purity;
pub mod registry;
pub mod sema;
pub mod types;

pub use options::{CompileOptions, TypePolicy, WorkflowSignature};
pub use sema::{Diagnostic, Severity};
