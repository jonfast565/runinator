//! Narrow API capabilities preserve authorization scope and API error classification.

mod function_artifacts;
pub use function_artifacts::FunctionArtifactSource;
mod function_exports;
pub use function_exports::FunctionExportResolver;
mod run_secrets;
pub use run_secrets::RunSecretReader;
mod run_files;
pub use run_files::RunFileSource;
mod execution_profiles;
pub use execution_profiles::ExecutionProfileSource;
mod artifact_content;
pub use artifact_content::ArtifactContentUploader;
mod workspace_objects;
pub use workspace_objects::WorkspaceObjectTransport;
mod workspace_checkouts;
pub use workspace_checkouts::WorkspaceCheckoutClient;
