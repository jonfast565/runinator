// the knobs a compile needs that the source text does not carry. these live in the sema crate
// because it is the lowest layer that reads them: the type passes need `type_policy` and
// `workflow_signatures`, and lowering needs the rest.

use std::path::PathBuf;

use runinator_models::functions::FunctionCatalogEntry;
use runinator_models::providers::ProviderMetadata;
use runinator_models::semver::SemVer;
use runinator_models::types::RuninatorType;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypePolicy {
    Strict,
    Permissive,
}

mod compile_options;
pub use compile_options::CompileOptions;

mod workflow_signature;
pub use workflow_signature::WorkflowSignature;
