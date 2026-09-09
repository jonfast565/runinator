//! Shared SQL implementation for portable workspace contents.
use super::*;
use runinator_models::workspaces::*;

impl<B> runinator_store::roles::durable_workspaces::WorkspaceRetentionStore for SqlStore<B>
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
    async fn workspace_pack_needed(
        &self,
        workspace: Uuid,
        scope: Uuid,
        pack: String,
    ) -> Result<bool, SendableError> {
        let count: i64 = sqlx::query_scalar(&self.render("SELECT (SELECT COUNT(*) FROM workspace_transfers WHERE workspace_id = ? AND token = ? AND state = 'running' AND lease_until > ?) + (SELECT COUNT(*) FROM workspace_objects WHERE workspace_id = ? AND pack_id = ?) + (SELECT COUNT(*) FROM workspace_checkouts WHERE workspace_id = ? AND id = ? AND leased_until > ?) + (SELECT COUNT(*) FROM workspace_gc_state WHERE workspace_id = ? AND token = ? AND lease_until > ?) + (SELECT COUNT(*) FROM workspace_readers WHERE workspace_id = ? AND lease_until > ?)"))
            .bind(workspace).bind(scope).bind(Utc::now().timestamp()).bind(workspace).bind(pack).bind(workspace).bind(scope).bind(Utc::now().timestamp()).bind(workspace).bind(scope).bind(Utc::now().timestamp()).bind(workspace).bind(Utc::now().timestamp()).fetch_one(self.pool()).await?;
        Ok(count > 0)
    }
    async fn workspace_gc_candidates(&self) -> Result<Vec<Uuid>, SendableError> {
        Ok(sqlx::query_scalar(&self.render("SELECT w.id FROM durable_workspaces w LEFT JOIN workspace_gc_state g ON g.workspace_id = w.id WHERE (g.workspace_id IS NULL OR g.last_revision < w.revision OR EXISTS (SELECT 1 FROM workspace_retired_packs p WHERE p.workspace_id = w.id)) AND (g.workspace_id IS NULL OR g.lease_until <= ?) ORDER BY w.updated_at LIMIT 20"))
            .bind(Utc::now().timestamp()).fetch_all(self.pool()).await?)
    }
    async fn claim_workspace_gc(
        &self,
        id: Uuid,
    ) -> Result<Option<WorkspaceGcLease>, SendableError> {
        let mut tx = self.pool().begin().await?;
        sqlx::query(&self.render("UPDATE durable_workspaces SET revision = revision WHERE id = ?"))
            .bind(id)
            .execute(&mut *tx)
            .await?;
        let revision: Option<i64> = sqlx::query_scalar(
            &self.render("SELECT revision FROM durable_workspaces WHERE id = ?"),
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(revision) = revision else {
            return Ok(None);
        };
        let active: i64 = sqlx::query_scalar(&self.render("SELECT COUNT(*) FROM workspace_checkouts WHERE workspace_id = ? AND writer = 1 AND leased_until > ?"))
            .bind(id).bind(Utc::now().timestamp()).fetch_one(&mut *tx).await?;
        if active > 0 {
            return Ok(None);
        }
        let imports: i64 = sqlx::query_scalar(&self.render("SELECT COUNT(*) FROM workspace_transfers WHERE workspace_id = ? AND importing = 1 AND state IN ('uploading', 'receiving', 'queued', 'running') AND expires_at > ?"))
            .bind(id).bind(Utc::now().timestamp()).fetch_one(&mut *tx).await?;
        if imports > 0 {
            return Ok(None);
        }
        let token = Uuid::new_v4();
        let sql = if self.dialect() == SqlDialect::MariaDb {
            "INSERT INTO workspace_gc_state (workspace_id, token) VALUES (?, ?) ON DUPLICATE KEY UPDATE workspace_id = workspace_id"
        } else {
            "INSERT INTO workspace_gc_state (workspace_id, token) VALUES (?, ?) ON CONFLICT (workspace_id) DO NOTHING"
        };
        sqlx::query(&self.render(sql))
            .bind(id)
            .bind(token)
            .execute(&mut *tx)
            .await?;
        let last: i64 = sqlx::query_scalar(
            &self.render("SELECT last_revision FROM workspace_gc_state WHERE workspace_id = ?"),
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        if last >= revision {
            return Ok(None);
        }
        let expires_at = Utc::now() + chrono::Duration::minutes(5);
        let changed = sqlx::query(&self.render("UPDATE workspace_gc_state SET token = ?, fence = fence + 1, lease_until = ? WHERE workspace_id = ? AND lease_until <= ?"))
            .bind(token).bind(expires_at.timestamp()).bind(id).bind(Utc::now().timestamp()).execute(&mut *tx).await?;
        if changed.affected() != 1 {
            return Ok(None);
        }
        let fence: i64 = sqlx::query_scalar(
            &self.render("SELECT fence FROM workspace_gc_state WHERE workspace_id = ?"),
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        let mut roots: Vec<String> = sqlx::query_scalar(&self.render("SELECT revision_id FROM workspace_snapshots WHERE workspace_id = ? AND deleted_at IS NULL AND snapshot_json <> 'null'"))
            .bind(id).fetch_all(&mut *tx).await?;
        let receipts: Vec<String> = sqlx::query_scalar(&self.render("SELECT r.receipt_json FROM workspace_receipts r JOIN workspace_checkouts c ON c.id = r.checkout_id JOIN workflow_effects e ON e.id = c.effect_id WHERE r.workspace_id = ? AND r.consumed = 0 AND e.status IN ('requested', 'running', 'input_required')"))
            .bind(id).fetch_all(&mut *tx).await?;
        for receipt in receipts {
            roots.push(
                serde_json::from_str::<WorkspaceReceipt>(&receipt)?
                    .snapshot
                    .revision_id,
            );
        }
        roots.sort();
        roots.dedup();
        sqlx::query(&self.render("DELETE FROM workspace_gc_objects WHERE workspace_id = ?"))
            .bind(id)
            .execute(&mut *tx)
            .await?;
        let object_count: i64 = sqlx::query_scalar(
            &self.render("SELECT COUNT(*) FROM workspace_objects WHERE workspace_id = ?"),
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(Some(WorkspaceGcLease {
            object_count: object_count as u64,
            workspace_id: id,
            token,
            fence,
            revision,
            expires_at,
            roots,
        }))
    }
    async fn renew_workspace_gc(&self, lease: WorkspaceGcLease) -> Result<bool, SendableError> {
        let changed = sqlx::query(&self.render("UPDATE workspace_gc_state SET lease_until = ? WHERE workspace_id = ? AND token = ? AND fence = ? AND lease_until > ?"))
            .bind((Utc::now() + chrono::Duration::minutes(5)).timestamp()).bind(lease.workspace_id).bind(lease.token).bind(lease.fence).bind(Utc::now().timestamp()).execute(self.pool()).await?;
        Ok(changed.affected() == 1)
    }
    async fn stage_workspace_gc(
        &self,
        lease: WorkspaceGcLease,
        objects: Vec<WorkspaceObjectLocation>,
    ) -> Result<(), SendableError> {
        if objects.len() > 1000 {
            return Err(runinator_models::errors::WORKSPACE_INVALID.error("GC batch exceeds 1000"));
        }
        let mut tx = self.pool().begin().await?;
        let locked = sqlx::query(&self.render("UPDATE workspace_gc_state SET lease_until = lease_until WHERE workspace_id = ? AND token = ? AND fence = ? AND lease_until > ?"))
            .bind(lease.workspace_id).bind(lease.token).bind(lease.fence).bind(Utc::now().timestamp()).execute(&mut *tx).await?;
        if locked.affected() != 1 {
            return Err(
                runinator_models::errors::WORKSPACE_CONFLICT.error("collection lease expired")
            );
        }
        for object in objects {
            sqlx::query(&self.render("INSERT INTO workspace_gc_objects (workspace_id, fence, object_id, pack_id, location_json) VALUES (?, ?, ?, ?, ?)"))
                .bind(lease.workspace_id).bind(lease.fence).bind(object.id.as_str()).bind(object.pack.as_str()).bind(serde_json::to_string(&object)?).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(())
    }
    async fn finish_workspace_gc(
        &self,
        lease: WorkspaceGcLease,
        repacked: bool,
    ) -> Result<(), SendableError> {
        let mut tx = self.pool().begin().await?;
        sqlx::query(&self.render("UPDATE durable_workspaces SET revision = revision WHERE id = ?"))
            .bind(lease.workspace_id)
            .execute(&mut *tx)
            .await?;
        let changed = sqlx::query(&self.render("UPDATE workspace_gc_state SET lease_until = 0, last_revision = ? WHERE workspace_id = ? AND token = ? AND fence = ? AND lease_until > ?"))
            .bind(lease.revision).bind(lease.workspace_id).bind(lease.token).bind(lease.fence).bind(Utc::now().timestamp()).execute(&mut *tx).await?;
        if changed.affected() != 1 {
            return Err(runinator_models::errors::WORKSPACE_CONFLICT.error("collection fence lost"));
        }
        if !repacked {
            tx.commit().await?;
            return Ok(());
        }
        sqlx::query(&self.render("INSERT INTO workspace_retired_packs (workspace_id, pack_id, delete_after) SELECT DISTINCT o.workspace_id, o.pack_id, ? FROM workspace_objects o WHERE o.workspace_id = ? AND NOT EXISTS (SELECT 1 FROM workspace_gc_objects g WHERE g.workspace_id = o.workspace_id AND g.fence = ? AND g.pack_id = o.pack_id) AND NOT EXISTS (SELECT 1 FROM workspace_retired_packs p WHERE p.workspace_id = o.workspace_id AND p.pack_id = o.pack_id)"))
            .bind(Utc::now().timestamp()).bind(lease.workspace_id).bind(lease.fence).execute(&mut *tx).await?;
        sqlx::query(&self.render("DELETE FROM workspace_objects WHERE workspace_id = ?"))
            .bind(lease.workspace_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query(&self.render("INSERT INTO workspace_objects (workspace_id, object_id, pack_id, location_json, created_at) SELECT workspace_id, object_id, pack_id, location_json, ? FROM workspace_gc_objects WHERE workspace_id = ? AND fence = ?"))
            .bind(Utc::now().timestamp()).bind(lease.workspace_id).bind(lease.fence).execute(&mut *tx).await?;
        sqlx::query(
            &self.render("DELETE FROM workspace_gc_objects WHERE workspace_id = ? AND fence = ?"),
        )
        .bind(lease.workspace_id)
        .bind(lease.fence)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }
    async fn expired_workspace_packs(&self, id: Uuid) -> Result<Vec<String>, SendableError> {
        Ok(sqlx::query_scalar(&self.render("SELECT p.pack_id FROM workspace_retired_packs p WHERE p.workspace_id = ? AND p.delete_after <= ? AND NOT EXISTS (SELECT 1 FROM workspace_readers r WHERE r.workspace_id = p.workspace_id AND r.lease_until > ?) AND NOT EXISTS (SELECT 1 FROM workspace_objects o WHERE o.workspace_id = p.workspace_id AND o.pack_id = p.pack_id) LIMIT 1000"))
            .bind(id).bind(Utc::now().timestamp()).bind(Utc::now().timestamp()).fetch_all(self.pool()).await?)
    }
    async fn finish_workspace_pack_cleanup(
        &self,
        id: Uuid,
        pack: String,
    ) -> Result<(), SendableError> {
        sqlx::query(
            &self.render(
                "DELETE FROM workspace_retired_packs WHERE workspace_id = ? AND pack_id = ?",
            ),
        )
        .bind(id)
        .bind(pack)
        .execute(self.pool())
        .await?;
        Ok(())
    }
    async fn pin_workspace_reader(
        &self,
        id: Uuid,
        version: i64,
    ) -> Result<WorkspaceReaderLease, SendableError> {
        let mut tx = self.pool().begin().await?;
        sqlx::query(&self.render(
            "UPDATE durable_workspaces SET revision = revision WHERE id = ? AND deleted_at IS NULL",
        ))
        .bind(id)
        .execute(&mut *tx)
        .await?;
        let present: Option<i64> = sqlx::query_scalar(&self.render("SELECT version FROM workspace_snapshots WHERE workspace_id = ? AND version = ? AND deleted_at IS NULL"))
            .bind(id).bind(version).fetch_optional(&mut *tx).await?;
        if present.is_none() {
            return Err(
                runinator_models::errors::WORKSPACE_MISSING.error("version is no longer retained")
            );
        }
        let lease = WorkspaceReaderLease {
            id: Uuid::new_v4(),
            workspace_id: id,
            version,
            expires_at: Utc::now() + chrono::Duration::minutes(5),
        };
        sqlx::query(&self.render("INSERT INTO workspace_readers (id, workspace_id, version, lease_until) VALUES (?, ?, ?, ?)"))
            .bind(lease.id).bind(id).bind(version).bind(lease.expires_at.timestamp()).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(lease)
    }
    async fn renew_workspace_reader(
        &self,
        lease: WorkspaceReaderLease,
    ) -> Result<bool, SendableError> {
        let changed = sqlx::query(&self.render(
            "UPDATE workspace_readers SET lease_until = ? WHERE id = ? AND lease_until > ?",
        ))
        .bind((Utc::now() + chrono::Duration::minutes(5)).timestamp())
        .bind(lease.id)
        .bind(Utc::now().timestamp())
        .execute(self.pool())
        .await?;
        Ok(changed.affected() == 1)
    }
    async fn release_workspace_reader(&self, id: Uuid) -> Result<(), SendableError> {
        sqlx::query(&self.render("DELETE FROM workspace_readers WHERE id = ?"))
            .bind(id)
            .execute(self.pool())
            .await?;
        Ok(())
    }
}
