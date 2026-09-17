//! packaged functions: immutable code published to the platform and invoked as ordinary actions.
//!
//! the shape is four nested things plus two references:
//!
//! ```text
//! FunctionPackage          "image-tools", owned by an org
//!   └── FunctionVersion    version 3, pinned to one artifact digest, immutable once published
//!         └── FunctionExport   "resize", with typed input and output
//!   └── FunctionAlias      "production" -> version 3, the one movable pointer
//! FunctionArtifact         the bytes, addressed by their sha-256
//! FunctionBinding          what a compiled workflow records so it keeps calling version 3
//! ```
//!
//! an alias points at a *version*, not an export, so every export in a package advances together —
//! a package is released as a unit, and a per-export alias would let two exports of one deploy
//! disagree about which code they are.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::providers::{ActionMetadata, ParameterMetadata, ResultMetadata};
use crate::validation::{
    LONG_TEXT_MAX, SHORT_TEXT_MAX, Validate, ValidationError, identifier, optional_text,
    required_text,
};
use crate::value::Value;

mod alias;
mod artifact;
mod binding;
mod export;
mod package;
mod version;

pub use alias::FunctionAlias;
pub use artifact::{
    ARTIFACT_MEDIA_TYPE, DIGEST_PREFIX, FunctionArtifact, digest_from_hex, is_valid_digest,
};
pub use binding::{FunctionBinding, FunctionInvocationContext};
pub use export::{FunctionExport, FunctionResourceLimits, FunctionRuntimeSpec};
pub use package::FunctionPackage;
pub use version::FunctionVersion;

/// the alias every package has unless an operator makes another. named rather than implied so the
/// CLI, the UI, and the invocation API all resolve the same default.
pub const DEFAULT_ALIAS: &str = "latest";

/// A version number reserved for a package that is being published by the same pack currently
/// compiling a workflow.
///
/// A compiled pack cannot know the database UUIDs or the monotonic version number that publish
/// will assign. The client therefore binds calls to deterministic *temporary* UUIDs and this
/// reserved version. The pack importer replaces those bindings with the newly published catalog
/// entries before it persists the workflow. Store implementations must never assign this value to
/// a real release.
pub const PROVISIONAL_FUNCTION_VERSION: i64 = i64::MAX;

/// the provider name a packaged function call lowers to. per-package authoring names
/// (`functions.image_tools`) are catalog entries; this is the one provider a worker resolves.
pub const FUNCTIONS_PROVIDER: &str = "functions";

/// the single action that provider advertises. the worker validates an action's function against
/// provider metadata before executing, so per-export names would be rejected — the export is named
/// by the binding instead.
pub const FUNCTIONS_INVOKE: &str = "invoke";

/// the prefix a per-package authoring provider carries in the catalog, e.g. `functions.image_tools`.
pub const FUNCTIONS_NAMESPACE_PREFIX: &str = "functions.";

/// the routing label a worker must advertise to run packaged functions. execution needs a container
/// runtime, which not every worker has, so an unlabelled pool must not receive these actions.
pub const FUNCTIONS_RUNNER_LABEL: &str = "functions";

/// the parameter names the worker injects for one invocation.
///
/// they live here rather than in the provider crate because two other places need them: the
/// provider advertises them, and the engine writes the same metadata into the catalog when a
/// package is published — publishing must not depend on a worker having started, which is the whole
/// point of a durable catalog. one definition, so the two cannot disagree.
pub const INVOKE_PACKAGE_PATH: &str = "package_path";
pub const INVOKE_HANDLER: &str = "handler";
pub const INVOKE_RUNTIME: &str = "runtime";
pub const INVOKE_LIMITS: &str = "limits";
pub const INVOKE_INPUT: &str = "input";
pub const INVOKE_CONTEXT: &str = "context";

/// the metadata for the one action the `functions` provider advertises.
pub fn invoke_action_metadata() -> ActionMetadata {
    use crate::types::RuninatorType;
    ActionMetadata::new(
        FUNCTIONS_INVOKE,
        "invoke a published packaged-function export",
    )
    .with_parameters(vec![
        // required of the *worker*, not of the author: staging fills these in before the provider
        // runs, so they are optional here — a compiled action carries only `input`, and validation
        // sees the action exactly as it was compiled.
        ParameterMetadata::optional(INVOKE_PACKAGE_PATH, RuninatorType::String),
        ParameterMetadata::optional(INVOKE_HANDLER, RuninatorType::String),
        ParameterMetadata::optional(INVOKE_RUNTIME, RuninatorType::Any),
        ParameterMetadata::optional(INVOKE_LIMITS, RuninatorType::Any),
        ParameterMetadata::optional(INVOKE_INPUT, RuninatorType::Any),
        ParameterMetadata::optional(INVOKE_CONTEXT, RuninatorType::Any),
    ])
}

/// the provider metadata for the runtime `functions` provider.
pub fn functions_provider_metadata() -> crate::providers::ProviderMetadata {
    crate::providers::ProviderMetadata {
        name: FUNCTIONS_PROVIDER.to_string(),
        actions: vec![invoke_action_metadata()],
        metadata: Default::default(),
    }
}

/// how a caller named the version it wants.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FunctionVersionRef {
    /// an alias, resolved at call time — so it follows a promotion.
    Alias(String),
    /// an exact version number, which no promotion moves.
    Exact(i64),
}

impl Default for FunctionVersionRef {
    fn default() -> Self {
        FunctionVersionRef::Alias(DEFAULT_ALIAS.to_string())
    }
}

#[cfg(test)]
#[path = "functions_tests.rs"]
mod tests;

mod function_catalog_entry;
pub use function_catalog_entry::FunctionCatalogEntry;

mod function_package_detail;
pub use function_package_detail::FunctionPackageDetail;

mod new_function_version;
pub use new_function_version::NewFunctionVersion;

mod new_function_package;
pub use new_function_package::NewFunctionPackage;

mod new_function_export;
pub use new_function_export::NewFunctionExport;

mod function_invocation_target;
pub use function_invocation_target::FunctionInvocationTarget;

mod function_adapter_workflow;
pub use function_adapter_workflow::FunctionAdapterWorkflow;
