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
}
