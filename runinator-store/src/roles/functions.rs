//! packaged functions: published code, its versions, exports, aliases, and artifacts.
//!
//! one of the role traits `DatabaseImpl` composes. bound on this directly when a caller only
//! needs this slice of the store.
//!
//! the write surface is deliberately coarse. `publish_function_version` takes a whole package plus
//! its exports and does the upsert, the version-number assignment, and the export insert together,
//! because those three are one atomic release: a version whose exports landed separately would be
//! visible to the catalog in a state no publisher ever described.

use std::future::Future;

use uuid::Uuid;

use runinator_models::{
    errors::SendableError,
    functions::{
        FunctionAdapterWorkflow, FunctionAlias, FunctionArtifact, FunctionCatalogEntry,
        FunctionExport, FunctionPackage, FunctionVersion, NewFunctionVersion,
    },
};

/// Persistence for packaged functions.
pub trait FunctionStore: Send + Sync + 'static {
    /// Record an artifact, or return the existing one when these bytes are already stored.
    ///
    /// Idempotent by digest: republishing identical bytes must not duplicate storage, which is the
    /// whole point of addressing an artifact by its content.
    fn upsert_function_artifact(
        &self,
        artifact: &FunctionArtifact,
    ) -> impl Future<Output = Result<FunctionArtifact, SendableError>> + Send;

    /// Fetch an artifact by digest.
    fn fetch_function_artifact(
        &self,
        digest: &str,
    ) -> impl Future<Output = Result<Option<FunctionArtifact>, SendableError>> + Send;

    /// Delete an artifact. Fails while any version still references it; an artifact whose bytes
    /// disappeared out from under a pinned version would make that version unrunnable.
    fn delete_function_artifact(
        &self,
        digest: &str,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    /// Publish one version: upsert the package, assign the next version number, insert the exports,
    /// and optionally move an alias — as one unit.
    fn publish_function_version(
        &self,
        request: &NewFunctionVersion,
    ) -> impl Future<Output = Result<FunctionVersion, SendableError>> + Send;

    /// Fetch every package, newest first.
    fn fetch_function_packages(
        &self,
    ) -> impl Future<Output = Result<Vec<FunctionPackage>, SendableError>> + Send;

    /// Fetch one package by its `(org, namespace, name)` identity.
    fn fetch_function_package(
        &self,
        org_id: Option<Uuid>,
        namespace: Option<&str>,
        name: &str,
    ) -> impl Future<Output = Result<Option<FunctionPackage>, SendableError>> + Send;

    /// Fetch one package by id.
    fn fetch_function_package_by_id(
        &self,
        package_id: Uuid,
    ) -> impl Future<Output = Result<Option<FunctionPackage>, SendableError>> + Send;

    /// Archive a package. Versions, exports, adapters, and artifacts remain for pinned snapshots.
    fn delete_function_package(
        &self,
        package_id: Uuid,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    /// Restore a previously archived package.
    fn restore_function_package(
        &self,
        package_id: Uuid,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    /// Fetch a package's versions, newest first.
    fn fetch_function_versions(
        &self,
        package_id: Uuid,
    ) -> impl Future<Output = Result<Vec<FunctionVersion>, SendableError>> + Send;

    /// Fetch one version by id.
    fn fetch_function_version(
        &self,
        version_id: Uuid,
    ) -> impl Future<Output = Result<Option<FunctionVersion>, SendableError>> + Send;

    /// Fetch one version by its number within a package.
    fn fetch_function_version_by_number(
        &self,
        package_id: Uuid,
        version: i64,
    ) -> impl Future<Output = Result<Option<FunctionVersion>, SendableError>> + Send;

    /// Fetch a version's exports.
    fn fetch_function_exports(
        &self,
        version_id: Uuid,
    ) -> impl Future<Output = Result<Vec<FunctionExport>, SendableError>> + Send;

    /// Fetch one export by id.
    fn fetch_function_export(
        &self,
        export_id: Uuid,
    ) -> impl Future<Output = Result<Option<FunctionExport>, SendableError>> + Send;

    /// Point an alias at a version, creating it if needed.
    fn set_function_alias(
        &self,
        package_id: Uuid,
        name: &str,
        version_id: Uuid,
    ) -> impl Future<Output = Result<FunctionAlias, SendableError>> + Send;

    /// Fetch a package's aliases.
    fn fetch_function_aliases(
        &self,
        package_id: Uuid,
    ) -> impl Future<Output = Result<Vec<FunctionAlias>, SendableError>> + Send;

    /// Resolve an alias to the version it names.
    fn fetch_function_alias(
        &self,
        package_id: Uuid,
        name: &str,
    ) -> impl Future<Output = Result<Option<FunctionAlias>, SendableError>> + Send;

    /// Delete an alias. The version it pointed at is untouched.
    fn delete_function_alias(
        &self,
        package_id: Uuid,
        name: &str,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    /// Artifacts no version references any more.
    ///
    /// Deleting a package leaves its artifacts behind, because they are addressed by content and a
    /// *different* package may have published the same bytes. This is what a retention sweep reads
    /// to find the ones nothing points at.
    fn fetch_unreferenced_function_artifacts(
        &self,
    ) -> impl Future<Output = Result<Vec<FunctionArtifact>, SendableError>> + Send;

    /// The flattened view of every published export: what the catalog is built from and what an
    /// offline compile is handed.
    ///
    /// Returns one entry per export of every version, with the aliases currently resolving to it.
    /// Callers that only want the current release filter by alias rather than asking for a
    /// different query, so a workflow pinned to an old version can still be type-checked.
    fn fetch_function_catalog(
        &self,
    ) -> impl Future<Output = Result<Vec<FunctionCatalogEntry>, SendableError>> + Send;

    /// Record the adapter workflow generated for an export.
    fn upsert_function_adapter_workflow(
        &self,
        export_id: Uuid,
        workflow_id: Uuid,
    ) -> impl Future<Output = Result<FunctionAdapterWorkflow, SendableError>> + Send;

    /// Fetch the adapter workflow for an export.
    fn fetch_function_adapter_workflow(
        &self,
        export_id: Uuid,
    ) -> impl Future<Output = Result<Option<FunctionAdapterWorkflow>, SendableError>> + Send;
}
