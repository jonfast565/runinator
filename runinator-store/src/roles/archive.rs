//! aging rows out to cold storage: marking candidates, claiming marks under a lease, and moving or dropping the marked rows.
//!
//! one of the role traits `DatabaseImpl` composes. bound on this directly when a caller only
//! needs this slice of the store.

use std::future::Future;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use runinator_models::errors::SendableError;

use crate::archive::{ArchiveMark, ArchiveRow, ArchiveTable};
// re-exported here so callers that reach for the contract at its historical path
// (`runinator_database::interfaces::*`) can import both halves from one place.
pub use crate::reducer_store::ReducerStore;

/// Core persistence operations for Runinator.
/// Aging rows out to cold storage: marking candidates, claiming marks under a lease, and moving or dropping the marked rows.
pub trait ArchiveStore: Send + Sync + 'static {
    /// Mark old rows that are eligible for archival. Marking is idempotent.
    fn mark_archive_candidates(
        &self,
        table: ArchiveTable,
        eligible_before: DateTime<Utc>,
        limit: i64,
    ) -> impl Future<Output = Result<u64, SendableError>> + Send;

    /// Claim archive marks for one archiver process under a short lease.
    fn claim_archive_marks(
        &self,
        archiver_id: String,
        now: DateTime<Utc>,
        lease_until: DateTime<Utc>,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<ArchiveMark>, SendableError>> + Send;

    /// Fetch source rows for claimed archive marks, rechecking eligibility at read time.
    fn fetch_archive_rows(
        &self,
        marks: Vec<ArchiveMark>,
    ) -> impl Future<Output = Result<Vec<ArchiveRow>, SendableError>> + Send;

    /// Delete archived source rows by exact table/id pairs.
    fn delete_archive_rows(
        &self,
        rows: Vec<ArchiveRow>,
    ) -> impl Future<Output = Result<u64, SendableError>> + Send;

    /// Mark archive ledger rows as archived after their source rows were deleted.
    fn complete_archive_marks(
        &self,
        mark_ids: Vec<Uuid>,
    ) -> impl Future<Output = Result<u64, SendableError>> + Send;

    /// Release archive marks after a failed archival attempt.
    fn fail_archive_marks(
        &self,
        mark_ids: Vec<Uuid>,
        error: String,
    ) -> impl Future<Output = Result<u64, SendableError>> + Send;

    /// Delete completed archive-ledger rows after their diagnostic retention window.
    fn prune_completed_archive_marks(
        &self,
        archived_before: DateTime<Utc>,
        limit: i64,
    ) -> impl Future<Output = Result<u64, SendableError>> + Send;

    /// Delete expired/revoked authentication sessions and consumed/expired enrollment tokens.
    fn prune_expired_security_records(
        &self,
        expired_before: DateTime<Utc>,
        limit: i64,
    ) -> impl Future<Output = Result<u64, SendableError>> + Send;

    /// Delete old cooldown keys after their deduplication window is no longer useful.
    fn prune_workflow_cooldowns(
        &self,
        used_before: DateTime<Utc>,
        limit: i64,
    ) -> impl Future<Output = Result<u64, SendableError>> + Send;
}
