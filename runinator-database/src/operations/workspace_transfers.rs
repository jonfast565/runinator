//! Shared SQL implementation for portable workspace contents.
use super::*;
use runinator_models::workspaces::*;

impl<B> runinator_store::roles::durable_workspaces::WorkspaceTransferStore for SqlStore<B>
where
    B: SqlBackend,
    for<'q> i64: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> bool: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> &'q str: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> String: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Uuid: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Option<Uuid>: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Option<i64>: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Option<String>: Encode<'q, B::Db> + Type<B::Db>,
    for<'r> i64: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> bool: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> String: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Uuid: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Option<i64>: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Option<String>: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Option<Uuid>: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Vec<u8>: Decode<'r, B::Db> + Type<B::Db>,
    usize: ColumnIndex<<B::Db as Database>::Row>,
    for<'c> &'c str: ColumnIndex<<B::Db as Database>::Row>,
    for<'q> <B::Db as Database>::Arguments<'q>: IntoArguments<'q, B::Db>,
    for<'c> &'c mut <B::Db as Database>::Connection: Executor<'c, Database = B::Db>,
    <B::Db as Database>::QueryResult: RowsAffected,
{
    async fn create_workspace_transfer(&self, job: WorkspaceTransfer) -> Result<(), SendableError> {
        job.limits.validate()?;
        let mut tx = self.pool().begin().await?;
        sqlx::query(&self.render("UPDATE durable_workspaces SET revision = revision WHERE id = ?"))
            .bind(job.workspace_id)
            .execute(&mut *tx)
            .await?;
        let head: Option<i64> = sqlx::query_scalar(&self.render(
            "SELECT head_version FROM durable_workspaces WHERE id = ? AND deleted_at IS NULL",
        ))
        .bind(job.workspace_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(head) = head else {
            return Err(runinator_models::errors::WORKSPACE_MISSING.error("workspace not found"));
        };
        let conflict: i64 = if job.importing {
            sqlx::query_scalar(&self.render("SELECT (SELECT COUNT(*) FROM workspace_checkouts WHERE workspace_id = ? AND leased_until > ?) + (SELECT COUNT(*) FROM workspace_transfers WHERE workspace_id = ? AND importing = 1 AND state IN ('uploading', 'receiving', 'queued', 'running') AND expires_at > ?) + (SELECT COUNT(*) FROM workspace_gc_state WHERE workspace_id = ? AND lease_until > ?)"))
                .bind(job.workspace_id).bind(Utc::now().timestamp()).bind(job.workspace_id).bind(Utc::now().timestamp()).bind(job.workspace_id).bind(Utc::now().timestamp()).fetch_one(&mut *tx).await?
        } else {
            let exists: i64 = sqlx::query_scalar(&self.render("SELECT COUNT(*) FROM workspace_snapshots WHERE workspace_id = ? AND version = ? AND deleted_at IS NULL")).bind(job.workspace_id).bind(job.version).fetch_one(&mut *tx).await?;
            i64::from(exists != 1)
        };
        if conflict != 0 || (job.importing && head != 0) {
            return Err(runinator_models::errors::WORKSPACE_CONFLICT
                .error("import requires an unused workspace; export requires a retained version"));
        }
        sqlx::query(&self.render("INSERT INTO workspace_transfers (id, workspace_id, version, importing, state, token, expires_at, job_json) VALUES (?, ?, ?, ?, ?, ?, ?, ?)"))
            .bind(job.id).bind(job.workspace_id).bind(job.version).bind(i64::from(job.importing)).bind(job.state.clone()).bind(job.token).bind(job.expires_at.timestamp()).bind(serde_json::to_string(&job)?).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }
    async fn fetch_workspace_transfer(
        &self,
        id: Uuid,
    ) -> Result<Option<WorkspaceTransfer>, SendableError> {
        let row = sqlx::query(&self.render("SELECT job_json, state, token, progress, archive_uri, error FROM workspace_transfers WHERE id = ?")).bind(id).fetch_optional(self.pool()).await?;
        row.map(|row| -> Result<_, SendableError> {
            let mut job: WorkspaceTransfer = serde_json::from_str(&row.try_get::<String, _>(0)?)?;
            job.state = row.try_get(1)?;
            job.token = row.try_get(2)?;
            job.bytes_processed = row.try_get::<i64, _>(3)? as u64;
            job.archive_uri = row.try_get(4)?;
            job.error = row.try_get(5)?;
            Ok(job)
        })
        .transpose()
    }
    async fn workspace_transfer_candidates(&self) -> Result<Vec<Uuid>, SendableError> {
        Ok(sqlx::query_scalar(&self.render("SELECT id FROM workspace_transfers WHERE (state = 'queued' OR (state = 'running' AND lease_until <= ?)) AND expires_at > ? ORDER BY expires_at LIMIT 10"))
            .bind(Utc::now().timestamp()).bind(Utc::now().timestamp()).fetch_all(self.pool()).await?)
    }
    async fn claim_workspace_transfer(
        &self,
        id: Uuid,
        uploading: bool,
    ) -> Result<Option<WorkspaceTransfer>, SendableError> {
        let token = Uuid::new_v4();
        let (pending, active) = if uploading {
            ("uploading", "receiving")
        } else {
            ("queued", "running")
        };
        let changed = sqlx::query(&self.render("UPDATE workspace_transfers SET state = ?, token = ?, lease_until = ?, progress = 0 WHERE id = ? AND (state = ? OR (state = ? AND lease_until <= ?)) AND expires_at > ?"))
            .bind(active).bind(token).bind((Utc::now() + chrono::Duration::minutes(5)).timestamp()).bind(id).bind(pending).bind(active).bind(Utc::now().timestamp()).bind(Utc::now().timestamp()).execute(self.pool()).await?;
        if changed.affected() != 1 {
            return Ok(None);
        }
        self.fetch_workspace_transfer(id).await
    }
    async fn progress_workspace_transfer(
        &self,
        job: WorkspaceTransfer,
        bytes: u64,
    ) -> Result<bool, SendableError> {
        let bytes = i64::try_from(bytes).map_err(|_| {
            runinator_models::errors::WORKSPACE_INVALID.error("transfer size overflow")
        })?;
        Ok(sqlx::query(&self.render("UPDATE workspace_transfers SET progress = ?, lease_until = ? WHERE id = ? AND token = ? AND state IN ('receiving', 'running') AND lease_until > ? AND expires_at > ?"))
            .bind(bytes).bind((Utc::now() + chrono::Duration::minutes(5)).timestamp()).bind(job.id).bind(job.token).bind(Utc::now().timestamp()).bind(Utc::now().timestamp()).execute(self.pool()).await?.affected() == 1)
    }
    async fn finish_workspace_transfer(
        &self,
        job: WorkspaceTransfer,
        state: String,
        archive: Option<String>,
        snapshot: Option<WorkspaceSnapshot>,
        error: Option<String>,
    ) -> Result<(), SendableError> {
        let mut tx = self.pool().begin().await?;
        sqlx::query(&self.render("UPDATE durable_workspaces SET revision = revision WHERE id = ?"))
            .bind(job.workspace_id)
            .execute(&mut *tx)
            .await?;
        let changed = sqlx::query(&self.render("UPDATE workspace_transfers SET state = ?, archive_uri = ?, error = ?, lease_until = 0 WHERE id = ? AND token = ? AND state IN ('receiving', 'running') AND lease_until > ? AND expires_at > ?"))
            .bind(state).bind(archive).bind(error).bind(job.id).bind(job.token).bind(Utc::now().timestamp()).bind(Utc::now().timestamp()).execute(&mut *tx).await?;
        if changed.affected() != 1 {
            return Err(runinator_models::errors::WORKSPACE_CONFLICT
                .error("transfer lease lost or cancelled"));
        }
        if let Some(snapshot) = snapshot {
            if !job.importing
                || snapshot.workspace_id != job.workspace_id
                || snapshot.version != 1
                || !matches!(&snapshot.origin, WorkspaceOrigin::Import { transfer_id, format } if *transfer_id == job.id && matches!(format.as_str(), "native" | "oci"))
            {
                return Err(
                    runinator_models::errors::WORKSPACE_INVALID.error("invalid import publication")
                );
            }
            let changed = sqlx::query(&self.render("UPDATE durable_workspaces SET head_version = 1, revision = revision + 1, updated_at = ? WHERE id = ? AND head_version = 0 AND deleted_at IS NULL"))
                .bind(Utc::now().timestamp()).bind(job.workspace_id).execute(&mut *tx).await?;
            if changed.affected() != 1 {
                return Err(runinator_models::errors::WORKSPACE_CONFLICT
                    .error("import destination is no longer empty"));
            }
            let row = sqlx::query(
                &self.render("SELECT metadata_json, revision FROM durable_workspaces WHERE id = ?"),
            )
            .bind(job.workspace_id)
            .fetch_one(&mut *tx)
            .await?;
            let mut identity: DurableWorkspace =
                serde_json::from_str(&row.try_get::<String, _>(0)?)?;
            identity.head_version = 1;
            identity.revision = row.try_get(1)?;
            identity.updated_at = Utc::now();
            sqlx::query(
                &self.render("UPDATE durable_workspaces SET metadata_json = ? WHERE id = ?"),
            )
            .bind(serde_json::to_string(&identity)?)
            .bind(job.workspace_id)
            .execute(&mut *tx)
            .await?;
            sqlx::query(&self.render("INSERT INTO workspace_snapshots (workspace_id, version, revision_id, snapshot_json) VALUES (?, 1, ?, ?)"))
                .bind(job.workspace_id).bind(snapshot.revision_id.clone()).bind(serde_json::to_string(&snapshot)?).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(())
    }
    async fn cancel_workspace_transfer(&self, id: Uuid) -> Result<bool, SendableError> {
        Ok(sqlx::query(&self.render("UPDATE workspace_transfers SET state = 'cancelled', token = ?, lease_until = 0 WHERE id = ? AND state IN ('uploading', 'receiving', 'queued', 'running')"))
            .bind(Uuid::new_v4()).bind(id).execute(self.pool()).await?.affected() == 1)
    }
    async fn stage_workspace_transfer(
        &self,
        job: WorkspaceTransfer,
        objects: Vec<WorkspaceObjectLocation>,
    ) -> Result<(), SendableError> {
        if objects.len() > 1000 {
            return Err(
                runinator_models::errors::WORKSPACE_INVALID.error("object batch exceeds 1000")
            );
        }
        let mut tx = self.pool().begin().await?;
        sqlx::query(
            &self.render("UPDATE durable_workspaces SET revision = revision + 1 WHERE id = ?"),
        )
        .bind(job.workspace_id)
        .execute(&mut *tx)
        .await?;
        let active: i64 = sqlx::query_scalar(&self.render("SELECT COUNT(*) FROM workspace_transfers WHERE id = ? AND token = ? AND state = 'running' AND importing = 1 AND lease_until > ? AND expires_at > ?"))
            .bind(job.id).bind(job.token).bind(Utc::now().timestamp()).bind(Utc::now().timestamp()).fetch_one(&mut *tx).await?;
        if active != 1 {
            return Err(runinator_models::errors::WORKSPACE_CONFLICT.error("transfer lease lost"));
        }
        for object in objects {
            let sql = if self.dialect() == SqlDialect::MariaDb {
                "INSERT IGNORE INTO workspace_objects (workspace_id, object_id, pack_id, location_json, created_at) VALUES (?, ?, ?, ?, ?)"
            } else {
                "INSERT INTO workspace_objects (workspace_id, object_id, pack_id, location_json, created_at) VALUES (?, ?, ?, ?, ?) ON CONFLICT (workspace_id, object_id) DO NOTHING"
            };
            sqlx::query(&self.render(sql))
                .bind(job.workspace_id)
                .bind(object.id.clone())
                .bind(object.pack.clone())
                .bind(serde_json::to_string(&object)?)
                .bind(Utc::now().timestamp())
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }
}
