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

/// one export as the rest of the system sees it: everything needed to type a call, pin it, and
/// dispatch it, flattened out of the package/version/export nesting.
///
/// this is the compile-time and catalog view. it is deliberately denormalised — a compiler running
/// offline against a pack's own sources has no database to join through.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionCatalogEntry {
    pub package_id: Uuid,
    pub package_name: String,
    /// the namespace qualifying the package name, if it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    pub version_id: Uuid,
    /// the package version this entry describes, monotonic per package.
    pub version: i64,
    pub export_id: Uuid,
    pub export_name: String,
    pub artifact_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub input: Vec<ParameterMetadata>,
    #[serde(default)]
    pub output: Vec<ResultMetadata>,
    /// the aliases currently resolving to this entry's version, e.g. `["production", "latest"]`.
    #[serde(default)]
    pub aliases: Vec<String>,
}

impl FunctionCatalogEntry {
    /// the provider name this export is authored under, e.g. `functions.image_tools`.
    ///
    /// the namespace is folded in so two orgs' packages of the same name stay distinguishable in a
    /// catalog that is keyed by provider name.
    pub fn provider_name(&self) -> String {
        match &self.namespace {
            Some(namespace) => format!(
                "{FUNCTIONS_NAMESPACE_PREFIX}{namespace}.{}",
                self.package_name
            ),
            None => format!("{FUNCTIONS_NAMESPACE_PREFIX}{}", self.package_name),
        }
    }

    /// the action metadata a compiler and the editor type this call against.
    pub fn action_metadata(&self) -> ActionMetadata {
        ActionMetadata {
            function_name: self.export_name.clone(),
            description: self.description.clone(),
            parameters: self.input.clone(),
            results: self.output.clone(),
            // packaged code runs a container; it is never reducer-evaluable in process.
            pure: false,
        }
    }

    /// the binding a compiled workflow records so it keeps calling exactly this version.
    pub fn binding(&self) -> FunctionBinding {
        FunctionBinding {
            package_id: self.package_id,
            package_name: self.package_name.clone(),
            namespace: self.namespace.clone(),
            version_id: self.version_id,
            version: self.version,
            export_id: self.export_id,
            export_name: self.export_name.clone(),
            artifact_digest: self.artifact_digest.clone(),
        }
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

/// a package plus everything published under it, as the API and UI read it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionPackageDetail {
    #[serde(flatten)]
    pub package: FunctionPackage,
    #[serde(default)]
    pub versions: Vec<FunctionVersion>,
    #[serde(default)]
    pub aliases: Vec<FunctionAlias>,
    /// exports of the version this package's default alias resolves to.
    #[serde(default)]
    pub exports: Vec<FunctionExport>,
}

/// a request to publish one version of a package.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewFunctionVersion {
    pub package: NewFunctionPackage,
    pub artifact_digest: String,
    /// the parsed manifest, kept verbatim so a republish can be compared against what was published.
    #[serde(default)]
    pub manifest: Value,
    pub runtime: FunctionRuntimeSpec,
    pub exports: Vec<NewFunctionExport>,
    /// move this alias onto the new version once it is published.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
}

/// the package half of a publish, which is upserted rather than required to exist.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewFunctionPackage {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub org_id: Option<Uuid>,
}

/// one export in a publish request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewFunctionExport {
    pub name: String,
    /// the entry point inside the package, e.g. `src.images.resize`.
    pub handler: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub input: Vec<ParameterMetadata>,
    #[serde(default)]
    pub output: Vec<ResultMetadata>,
    #[serde(default)]
    pub limits: FunctionResourceLimits,
}

/// everything a worker needs to run one export, resolved from its id.
///
/// the binding on a compiled action pins *which* code to run; this is *how* to run it. keeping the
/// handler, runtime, and limits here rather than on the binding keeps a compiled workflow small and
/// keeps one published fact in one place — a version is immutable, so resolving it is a cache hit
/// after the first call rather than a per-invocation round trip.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionInvocationTarget {
    pub package_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    pub version: i64,
    pub artifact_digest: String,
    pub runtime: FunctionRuntimeSpec,
    pub export: FunctionExport,
}

/// an adapter workflow generated for one export, so a direct http invocation runs through the same
/// reducer path a workflow call does.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionAdapterWorkflow {
    pub id: Uuid,
    pub export_id: Uuid,
    pub workflow_id: Uuid,
    pub created_at: DateTime<Utc>,
}

#[cfg(test)]
#[path = "functions_tests.rs"]
mod tests;
