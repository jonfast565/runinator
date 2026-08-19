//! the authored artefacts a run executes: workflow and pipeline definitions, their org ownership, and the provider catalog.
//!
//! one of the role traits `DatabaseImpl` composes. bound on this directly when a caller only
//! needs this slice of the store.

use std::future::Future;

use uuid::Uuid;

use runinator_models::value::Value;
use runinator_models::{
    errors::SendableError,
    pipelines::{Pipeline, PipelineRun},
    revisions::WorkflowRevision,
    workflows::WorkflowDefinition,
};

// re-exported here so callers that reach for the contract at its historical path
// (`runinator_database::interfaces::*`) can import both halves from one place.
pub use crate::reducer_store::ReducerStore;

/// Core persistence operations for Runinator.
/// The authored artefacts a run executes: workflow and pipeline definitions, their org ownership, and the provider catalog.
pub trait DefinitionStore: Send + Sync + 'static {
    /// Create or update a workflow definition.
    fn upsert_workflow(
        &self,
        workflow: &WorkflowDefinition,
    ) -> impl Future<Output = Result<WorkflowDefinition, SendableError>> + Send;

    /// Insert a workflow as a new row, ignoring any id and never updating an existing one.
    /// Used to duplicate a workflow into a sibling version that shares its name.
    fn insert_workflow(
        &self,
        workflow: &WorkflowDefinition,
    ) -> impl Future<Output = Result<WorkflowDefinition, SendableError>> + Send;

    /// Fetch all workflow definitions.
    fn fetch_workflows(
        &self,
    ) -> impl Future<Output = Result<Vec<WorkflowDefinition>, SendableError>> + Send;

    /// Fetch the ids of every workflow owned by an organization. lightweight lookup used to compose
    /// org-scoped visibility without loading full definitions.
    fn fetch_workflow_ids_for_org(
        &self,
        org_id: Uuid,
    ) -> impl Future<Output = Result<Vec<Uuid>, SendableError>> + Send;

    /// Reassign a workflow's owning organization (`None` makes it platform-global).
    fn set_workflow_org(
        &self,
        workflow_id: Uuid,
        org_id: Option<Uuid>,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Delete a workflow and its associated metadata.
    fn delete_workflow(
        &self,
        workflow_id: Uuid,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Append an immutable revision capturing an accepted definition.
    ///
    /// The revision number is assigned by the store, not the caller. Returns `None` when the
    /// incoming definition is identical to the workflow's current head revision, so a repeated
    /// pack apply does not mint an unbroken run of identical rows.
    fn insert_workflow_revision(
        &self,
        revision: &WorkflowRevision,
    ) -> impl Future<Output = Result<Option<WorkflowRevision>, SendableError>> + Send;

    /// Fetch a workflow's revisions, newest first, capped at `limit`.
    fn fetch_workflow_revisions(
        &self,
        workflow_id: Uuid,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<WorkflowRevision>, SendableError>> + Send;

    /// Fetch one revision by its per-workflow sequence number.
    fn fetch_workflow_revision(
        &self,
        workflow_id: Uuid,
        revision: i64,
    ) -> impl Future<Output = Result<Option<WorkflowRevision>, SendableError>> + Send;

    /// Create or update a pipeline instance.
    fn upsert_pipeline(
        &self,
        pipeline: &Pipeline,
    ) -> impl Future<Output = Result<Pipeline, SendableError>> + Send;

    /// Fetch all pipeline instances.
    fn fetch_pipelines(&self) -> impl Future<Output = Result<Vec<Pipeline>, SendableError>> + Send;

    /// Delete a pipeline instance.
    fn delete_pipeline(
        &self,
        pipeline_id: Uuid,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Fetch the ids of every pipeline owned by an organization (org-scoped visibility).
    fn fetch_pipeline_ids_for_org(
        &self,
        org_id: Uuid,
    ) -> impl Future<Output = Result<Vec<Uuid>, SendableError>> + Send;

    /// Reassign a pipeline's owning organization (`None` makes it platform-global).
    fn set_pipeline_org(
        &self,
        pipeline_id: Uuid,
        org_id: Option<Uuid>,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Fetch the most recent pipeline runs, newest first, capped at `limit`.
    fn fetch_recent_pipeline_runs(
        &self,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<PipelineRun>, SendableError>> + Send;

    /// Fetch all runs for a specific pipeline.
    fn fetch_pipeline_runs_for_pipeline(
        &self,
        pipeline_id: Uuid,
    ) -> impl Future<Output = Result<Vec<PipelineRun>, SendableError>> + Send;

    /// Permanently delete a pipeline run and all member workflow-run history.
    fn delete_pipeline_run(
        &self,
        pipeline_run_id: Uuid,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Create or update a generic catalog item.
    fn upsert_catalog_item(
        &self,
        item: Value,
    ) -> impl Future<Output = Result<Value, SendableError>> + Send;

    /// Fetch catalog items, optionally filtered by type.
    fn fetch_catalog_items(
        &self,
        item_type: Option<String>,
    ) -> impl Future<Output = Result<Vec<Value>, SendableError>> + Send;

    /// Fetch a single catalog item by its URI.
    fn fetch_catalog_item(
        &self,
        uri: String,
    ) -> impl Future<Output = Result<Option<Value>, SendableError>> + Send;

    /// Delete a catalog item by its URI. Returns false when there was nothing to delete.
    ///
    /// Needed because some catalog entries are *mirrors* of a row that can be deleted — a packaged
    /// function's `functions.<pkg>` provider metadata outlives its package otherwise, advertising
    /// exports nothing can run.
    fn delete_catalog_item(
        &self,
        uri: String,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;
}
