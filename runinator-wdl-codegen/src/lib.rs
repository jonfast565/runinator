// runinator-wdl-codegen: the mapping between the wdl ast and the runtime json workflow model,
// in both directions. `lower` turns an analyzed ast into a `WorkflowDefinition`; `decompile`
// turns a `WorkflowDefinition` back into wdl source text.
//
// the two share no code — decompile emits text directly rather than building an ast — but they
// share a contract: every node kind's parameters must survive a round trip. they live together
// so that contract has one owner. the round-trip assertions themselves are in `runinator-wdl`,
// which is the first crate that can see parse, lower, decompile, and format at once.

pub mod decompile;
pub mod lower;

pub use decompile::{DecompileOptions, decompile_definition};
pub use lower::lower_document;
