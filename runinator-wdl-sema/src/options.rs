// the knobs a compile needs that the source text does not carry. these live in the sema crate
// because it is the lowest layer that reads them: the type passes need `type_policy` and
// `workflow_signatures`, and lowering needs the rest.

use std::path::PathBuf;

use runinator_models::providers::ProviderMetadata;
use runinator_models::semver::SemVer;
use runinator_models::types::RuninatorType;

/// options that fill in the WorkflowDefinition fields that the source does not carry.
#[derive(Debug, Clone)]
pub struct CompileOptions {
    pub enabled: bool,
    /// fallback version when the source omits `vN`.
    pub default_version: SemVer,
    /// directory used to resolve `file("...")` includes.
    pub source_dir: Option<PathBuf>,
    /// provider metadata available while compiling, used to infer action output types.
    pub providers: Vec<ProviderMetadata>,
    /// strictness for author-time type diagnostics.
    pub type_policy: TypePolicy,
    /// pack-local or caller-supplied workflow signatures used to type subflow calls.
    pub workflow_signatures: Vec<WorkflowSignature>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypePolicy {
    Strict,
    Permissive,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WorkflowSignature {
    pub name: String,
    pub input: RuninatorType,
    pub output: RuninatorType,
}

impl Default for CompileOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            default_version: SemVer::default(),
            source_dir: None,
            providers: Vec::new(),
            type_policy: TypePolicy::Strict,
            workflow_signatures: Vec::new(),
        }
    }
}
