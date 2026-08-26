//! Durable metadata for user-uploaded workflow files.

use std::future::Future;

use runinator_models::{errors::SendableError, files::StoredFile};
use uuid::Uuid;

pub trait FileStore: Send + Sync + 'static {
    fn insert_file(
        &self,
        file: &StoredFile,
    ) -> impl Future<Output = Result<StoredFile, SendableError>> + Send;
    fn fetch_file(
        &self,
        id: Uuid,
    ) -> impl Future<Output = Result<Option<StoredFile>, SendableError>> + Send;
    fn list_library_files(
        &self,
        org_id: Option<Uuid>,
    ) -> impl Future<Output = Result<Vec<StoredFile>, SendableError>> + Send;
    fn next_library_revision(
        &self,
        org_id: Option<Uuid>,
        path: &str,
    ) -> impl Future<Output = Result<i64, SendableError>> + Send;
    fn claim_staged_files(
        &self,
        ids: &[Uuid],
        org_id: Option<Uuid>,
        owner_id: Option<Uuid>,
        workflow_run_id: Uuid,
    ) -> impl Future<Output = Result<Vec<StoredFile>, SendableError>> + Send;
    fn archive_file(
        &self,
        id: Uuid,
        org_id: Option<Uuid>,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;
}
