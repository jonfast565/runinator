//! standalone task runs — the `runs` table and its chunks and artifacts.
//!
//! one of the role traits `DatabaseImpl` composes. these are runs of a single task, recorded
//! outside the workflow vm; a workflow's own execution history is continuations, effects, and
//! journal entries instead.

use std::future::Future;

use uuid::Uuid;

use runinator_models::{
    errors::SendableError,
    runs::{NewRunArtifact, NewRunChunk, RunArtifact, RunChunk, RunStatus, RunSummary},
    value::Value,
};

// re-exported here so callers that reach for the contract at its historical path
// (`runinator_database::interfaces::*`) can import both halves from one place.
pub use crate::runtime_store::RuntimeStore;

/// Core persistence operations for Runinator.
/// The standalone task-run model (`runs`, chunks, artifacts) that predates workflow runs and is still served over the compatibility endpoints.
pub trait TaskRunStore: Send + Sync + 'static {
    /// Fetch all runs filtered by their current status.
    fn fetch_runs_by_status(
        &self,
        status: RunStatus,
    ) -> impl Future<Output = Result<Vec<RunSummary>, SendableError>> + Send;

    /// Update the status and output of a specific run.
    fn update_run_status(
        &self,
        run_id: Uuid,
        status: RunStatus,
        output_json: Option<Value>,
        message: Option<String>,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Append a log chunk to an active run.
    fn append_run_chunk(
        &self,
        run_id: Uuid,
        chunk: &NewRunChunk,
    ) -> impl Future<Output = Result<RunChunk, SendableError>> + Send;

    /// Fetch log chunks for a run with pagination.
    fn fetch_run_chunks(
        &self,
        run_id: Uuid,
        cursor: Option<i64>,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<RunChunk>, SendableError>> + Send;

    /// Associate a new artifact with a run.
    fn add_run_artifact(
        &self,
        run_id: Uuid,
        artifact: &NewRunArtifact,
    ) -> impl Future<Output = Result<RunArtifact, SendableError>> + Send;

    /// Fetch all artifacts produced by a specific run.
    fn fetch_run_artifacts(
        &self,
        run_id: Uuid,
    ) -> impl Future<Output = Result<Vec<RunArtifact>, SendableError>> + Send;

    /// Fetch every artifact across all runs, most-recent first.
    fn fetch_all_artifacts(
        &self,
    ) -> impl Future<Output = Result<Vec<RunArtifact>, SendableError>> + Send;

    /// Fetch a single artifact by its identifier.
    fn fetch_artifact(
        &self,
        artifact_id: Uuid,
    ) -> impl Future<Output = Result<Option<RunArtifact>, SendableError>> + Send;

    /// Delete a run artifact row; returns true when a row was removed.
    fn delete_artifact(
        &self,
        artifact_id: Uuid,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;
}
