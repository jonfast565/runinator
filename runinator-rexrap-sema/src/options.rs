// the knobs a compile needs that the source text does not carry. these live in the sema crate
// because it is the lowest layer that reads them: the type passes need `type_policy` and
// `workflow_signatures`, and lowering needs the rest.

use std::path::PathBuf;

use runinator_models::functions::FunctionCatalogEntry;
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
    /// published packaged-function exports a `functions.<pkg>.<export>(...)` call may name.
    ///
    /// one list rather than two: the `functions.<pkg>` provider metadata the type passes check
    /// against is *derived* from this ([`Self::function_providers`]), so a caller cannot supply a
    /// catalog the compiler types against but cannot bind, or the reverse.
    pub functions: Vec<FunctionCatalogEntry>,
    /// emit `invocation` nodes running compiled bytecode instead of `std.run`/`std.exec` nodes
    /// carrying a statement tree.
    ///
    /// off by default, and deliberately so: it changes what a compiled definition *is*, and a
    /// runtime holding in-flight runs against the old shape has to be drained and migrated before
    /// it can execute the new one. flipping this is a deployment decision, not a compile-time
    /// preference, which is why it is a flag a caller sets rather than a version the compiler picks.
    pub emit_invocations: bool,
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
            functions: Vec::new(),
            emit_invocations: false,
        }
    }
}

impl CompileOptions {
    /// the synthetic `functions.<pkg>` providers this option set's catalog implies.
    ///
    /// grouped by authoring provider name, one action per export. an export published in several
    /// versions contributes one action — the newest, since that is what an unversioned call
    /// resolves to — so the type checker sees exactly what lowering will bind.
    pub fn function_providers(&self) -> Vec<ProviderMetadata> {
        let mut by_provider: std::collections::BTreeMap<String, Vec<&FunctionCatalogEntry>> =
            std::collections::BTreeMap::new();
        for entry in &self.functions {
            by_provider
                .entry(entry.provider_name())
                .or_default()
                .push(entry);
        }
        by_provider
            .into_iter()
            .map(|(name, entries)| {
                let mut newest: std::collections::BTreeMap<&str, &FunctionCatalogEntry> =
                    std::collections::BTreeMap::new();
                for entry in entries {
                    newest
                        .entry(entry.export_name.as_str())
                        .and_modify(|current| {
                            if entry.version > current.version {
                                *current = entry;
                            }
                        })
                        .or_insert(entry);
                }
                ProviderMetadata {
                    name,
                    actions: newest
                        .values()
                        .map(|entry| entry.action_metadata())
                        .collect(),
                    metadata: Default::default(),
                }
            })
            .collect()
    }

    /// every provider a compile types against: the caller's plus the synthetic function ones.
    pub fn all_providers(&self) -> Vec<ProviderMetadata> {
        let mut providers = self.providers.clone();
        providers.extend(self.function_providers());
        providers
    }

    /// the catalog entry an unversioned `functions.<pkg>.<export>` call resolves to.
    ///
    /// the newest published version wins. a compiled workflow then records *that* version in its
    /// binding, so the resolution happens once at compile time and a later publish never changes
    /// what an existing workflow calls.
    pub fn resolve_function(&self, provider: &str, export: &str) -> Option<&FunctionCatalogEntry> {
        self.functions
            .iter()
            .filter(|entry| entry.provider_name() == provider && entry.export_name == export)
            .max_by_key(|entry| entry.version)
    }
}
