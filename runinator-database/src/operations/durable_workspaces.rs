//! Shared SQL implementation for portable workspace contents.
use super::*;
use runinator_models::workspaces::*;

impl<B> DurableWorkspaceStore for SqlStore<B>
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
    async fn create_durable_workspace(
        &self,
        workspace: DurableWorkspace,
        ownership: ResourceOwnership,
    ) -> Result<DurableWorkspace, SendableError> {
        let mut tx = self.pool().begin().await?;
        let tenant = workspace
            .org_id
            .map(|id| id.to_string())
            .unwrap_or_default();
        let sql = if self.dialect() == SqlDialect::MariaDb {
            "INSERT INTO durable_workspaces (id, org_id, updated_at, tenant_key, workspace_key, metadata_json) VALUES (?, ?, ?, ?, ?, ?) ON DUPLICATE KEY UPDATE id = id"
        } else {
            "INSERT INTO durable_workspaces (id, org_id, updated_at, tenant_key, workspace_key, metadata_json) VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT (tenant_key, workspace_key) DO NOTHING"
        };
        sqlx::query(&self.render(sql))
            .bind(workspace.id)
            .bind(workspace.org_id)
            .bind(workspace.updated_at.timestamp())
            .bind(tenant)
            .bind(workspace.key.as_str())
            .bind(serde_json::to_string(&workspace)?)
            .execute(&mut *tx)
            .await?;
        let actual: Uuid = sqlx::query_scalar(&self.render(
            "SELECT id FROM durable_workspaces WHERE tenant_key = ? AND workspace_key = ?",
        ))
        .bind(
            workspace
                .org_id
                .map(|id| id.to_string())
                .unwrap_or_default(),
        )
        .bind(workspace.key.as_str())
        .fetch_one(&mut *tx)
        .await?;
        if actual == workspace.id {
            sqlx::query(&self.render("INSERT INTO resource_ownership (resource_type, resource_id, tenant_scope_kind, tenant_scope_id, owner_scope_kind, owner_scope_id, created_by, authz_version, created_at, updated_at) VALUES ('workspace', ?, ?, ?, ?, ?, ?, 1, ?, ?)"))
                .bind(workspace.id).bind(ownership.tenant.kind.as_str()).bind(ownership.tenant.id)
                .bind(ownership.owner.kind.as_str()).bind(ownership.owner.id).bind(ownership.created_by)
                .bind(workspace.created_at.timestamp()).bind(workspace.updated_at.timestamp()).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        self.resolve_durable_workspace(workspace.org_id, workspace.key)
            .await?
            .ok_or_else(|| {
                crate::errors::WORKFLOW_VM_CORRUPT_STATE.error("workspace key is deleted")
            })
    }
    async fn resolve_durable_workspace(
        &self,
        org_id: Option<Uuid>,
        key: String,
    ) -> Result<Option<DurableWorkspace>, SendableError> {
        let json: Option<String> = sqlx::query_scalar(&self.render("SELECT metadata_json FROM durable_workspaces WHERE tenant_key = ? AND workspace_key = ? AND deleted_at IS NULL"))
            .bind(org_id.map(|id| id.to_string()).unwrap_or_default()).bind(key).fetch_optional(self.pool()).await?;
        json.map(|s| serde_json::from_str(&s).map_err(Into::into))
            .transpose()
    }
    async fn fetch_durable_workspace(
        &self,
        id: Uuid,
    ) -> Result<Option<DurableWorkspace>, SendableError> {
        let json: Option<String> = sqlx::query_scalar(&self.render(
            "SELECT metadata_json FROM durable_workspaces WHERE id = ? AND deleted_at IS NULL",
        ))
        .bind(id)
        .fetch_optional(self.pool())
        .await?;
        json.map(|s| serde_json::from_str(&s).map_err(Into::into))
            .transpose()
    }
    async fn list_durable_workspaces(
        &self,
        org_id: Option<Uuid>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<DurableWorkspace>, SendableError> {
        let rows: Vec<String> = sqlx::query_scalar(&self.render("SELECT metadata_json FROM durable_workspaces WHERE tenant_key = ? AND deleted_at IS NULL ORDER BY workspace_key LIMIT ? OFFSET ?"))
            .bind(org_id.map(|id| id.to_string()).unwrap_or_default()).bind(limit.clamp(1, 200)).bind(offset.max(0)).fetch_all(self.pool()).await?;
        rows.into_iter()
            .map(|s| serde_json::from_str(&s).map_err(Into::into))
            .collect()
    }
    async fn fetch_workspace_snapshot(
        &self,
        id: Uuid,
        version: i64,
    ) -> Result<Option<WorkspaceSnapshot>, SendableError> {
        let json: Option<String> = sqlx::query_scalar(&self.render("SELECT snapshot_json FROM workspace_snapshots WHERE workspace_id = ? AND version = ? AND deleted_at IS NULL"))
            .bind(id).bind(version).fetch_optional(self.pool()).await?;
        json.map(|s| serde_json::from_str(&s).map_err(Into::into))
            .transpose()
    }
    async fn list_workspace_snapshots(
        &self,
        id: Uuid,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<WorkspaceSnapshot>, SendableError> {
        let rows: Vec<String> = sqlx::query_scalar(&self.render("SELECT snapshot_json FROM workspace_snapshots WHERE workspace_id = ? AND deleted_at IS NULL ORDER BY version DESC LIMIT ? OFFSET ?"))
            .bind(id).bind(limit.clamp(1, 200)).bind(offset.max(0)).fetch_all(self.pool()).await?;
        rows.into_iter()
            .map(|s| serde_json::from_str(&s).map_err(Into::into))
            .collect()
    }

    async fn acquire_workspace_checkout(
        &self,
        request: WorkspaceAcquire,
    ) -> Result<WorkspaceAcquisition, SendableError> {
        let mut tx = self.pool().begin().await?;
        // an update locks the identity on every dialect, including sqlite.
        let changed = sqlx::query(&self.render("UPDATE durable_workspaces SET revision = revision + 1 WHERE id = ? AND deleted_at IS NULL"))
            .bind(request.workspace_id).execute(&mut *tx).await?;
        if changed.affected() == 0 {
            tx.rollback().await?;
            return Ok(WorkspaceAcquisition::Missing);
        }
        let row = sqlx::query(
            &self.render("SELECT head_version, revision FROM durable_workspaces WHERE id = ?"),
        )
        .bind(request.workspace_id)
        .fetch_one(&mut *tx)
        .await?;
        let head: i64 = row.try_get("head_version")?;
        let fence: i64 = row.try_get("revision")?;
        let previous: Option<String> = sqlx::query_scalar(&self.render("SELECT checkout_json FROM workspace_checkouts WHERE workspace_id = ? AND effect_id = ? AND attempt = ? AND leased_until > ?"))
            .bind(request.workspace_id).bind(request.effect_id).bind(i64::from(request.attempt)).bind(request.now.timestamp()).fetch_optional(&mut *tx).await?;
        if let Some(previous) = previous {
            let checkout: WorkspaceCheckout = serde_json::from_str(&previous)?;
            if checkout.leased_until > request.now {
                tx.rollback().await?;
                return Ok(WorkspaceAcquisition::Acquired { checkout });
            }
        }
        let pinned: Option<i64> = sqlx::query_scalar(&self.render("SELECT base_version FROM workspace_checkouts WHERE workspace_id = ? AND effect_id = ? ORDER BY attempt LIMIT 1"))
            .bind(request.workspace_id).bind(request.effect_id).fetch_optional(&mut *tx).await?;
        let base = request.version.or(pinned).unwrap_or(head);
        if base < 0 || base > head || (request.access == WorkspaceAccess::Write && base != head) {
            tx.rollback().await?;
            return Ok(WorkspaceAcquisition::Conflict);
        }
        if base > 0 {
            let present: Option<i64> = sqlx::query_scalar(&self.render("SELECT version FROM workspace_snapshots WHERE workspace_id = ? AND version = ? AND deleted_at IS NULL"))
                .bind(request.workspace_id).bind(base).fetch_optional(&mut *tx).await?;
            if present.is_none() {
                tx.rollback().await?;
                return Ok(WorkspaceAcquisition::Missing);
            }
        }
        if request.access == WorkspaceAccess::Write {
            let count: i64 = sqlx::query_scalar(&self.render("SELECT COUNT(*) FROM workspace_checkouts WHERE workspace_id = ? AND writer = 1 AND leased_until > ?"))
                .bind(request.workspace_id).bind(request.now.timestamp()).fetch_one(&mut *tx).await?;
            if count > 0 {
                tx.rollback().await?;
                return Ok(WorkspaceAcquisition::Busy);
            }
        }
        sqlx::query(&self.render("DELETE FROM workspace_checkouts WHERE workspace_id = ? AND effect_id = ? AND attempt = ? AND leased_until <= ?"))
            .bind(request.workspace_id).bind(request.effect_id).bind(i64::from(request.attempt)).bind(request.now.timestamp()).execute(&mut *tx).await?;
        let checkout = WorkspaceCheckout {
            id: Uuid::now_v7(),
            workspace_id: request.workspace_id,
            workflow_run_id: request.workflow_run_id,
            effect_id: request.effect_id,
            attempt: request.attempt,
            base_version: base,
            access: request.access,
            fence,
            leased_until: request.leased_until,
        };
        sqlx::query(&self.render("INSERT INTO workspace_checkouts (id, workspace_id, effect_id, attempt, base_version, writer, fence, leased_until, checkout_json) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"))
            .bind(checkout.id).bind(checkout.workspace_id).bind(checkout.effect_id).bind(i64::from(checkout.attempt))
            .bind(base).bind(i64::from(checkout.access == WorkspaceAccess::Write)).bind(fence)
            .bind(checkout.leased_until.timestamp()).bind(serde_json::to_string(&checkout)?).execute(&mut *tx).await?;
        if base > 0 {
            self.pin_workspace_version(
                &mut tx,
                checkout.workspace_id,
                base,
                checkout.workflow_run_id,
            )
            .await?;
        }
        tx.commit().await?;
        Ok(WorkspaceAcquisition::Acquired { checkout })
    }
    async fn release_workspace_checkout(
        &self,
        id: Uuid,
        fence: i64,
    ) -> Result<bool, SendableError> {
        let result =
            sqlx::query(&self.render(
                "UPDATE workspace_checkouts SET leased_until = 0 WHERE id = ? AND fence = ?",
            ))
            .bind(id)
            .bind(fence)
            .execute(self.pool())
            .await?;
        Ok(result.affected() > 0)
    }

    async fn fetch_workspace_checkout(
        &self,
        id: Uuid,
    ) -> Result<Option<WorkspaceCheckout>, SendableError> {
        let json: Option<String> = sqlx::query_scalar(&self.render(
            "SELECT checkout_json FROM workspace_checkouts WHERE id = ? AND leased_until > ?",
        ))
        .bind(id)
        .bind(Utc::now().timestamp())
        .fetch_optional(self.pool())
        .await?;
        json.map(|s| serde_json::from_str(&s).map_err(Into::into))
            .transpose()
    }
    async fn delete_durable_workspace(
        &self,
        id: Uuid,
        version: Option<i64>,
    ) -> Result<bool, SendableError> {
        let mut tx = self.pool().begin().await?;
        let changed = sqlx::query(&self.render("UPDATE durable_workspaces SET revision = revision + 1 WHERE id = ? AND deleted_at IS NULL"))
            .bind(id).execute(&mut *tx).await?;
        if changed.affected() == 0 {
            tx.rollback().await?;
            return Ok(false);
        }
        let head: i64 = sqlx::query_scalar(
            &self.render("SELECT head_version FROM durable_workspaces WHERE id = ?"),
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        if version == Some(head) || version.is_some_and(|v| v < 1) {
            return Err(runinator_models::errors::WORKSPACE_CONFLICT
                .error("cannot delete the current head"));
        }
        let active: i64 = sqlx::query_scalar(&self.render("SELECT COUNT(*) FROM workspace_checkouts WHERE workspace_id = ? AND leased_until > ? AND (? IS NULL OR base_version = ?)"))
            .bind(id).bind(Utc::now().timestamp()).bind(version).bind(version).fetch_one(&mut *tx).await?;
        if active > 0 {
            return Err(runinator_models::errors::WORKSPACE_CONFLICT.error("workspace is in use"));
        }
        let pinned: i64 = sqlx::query_scalar(&self.render("SELECT COUNT(*) FROM workspace_pins p JOIN workflow_runs r ON r.id = p.workflow_run_id LEFT JOIN pipeline_runs pr ON pr.id = r.pipeline_run_id WHERE p.workspace_id = ? AND (? IS NULL OR p.version = ?) AND (r.finished_at IS NULL OR (pr.id IS NOT NULL AND pr.finished_at IS NULL))"))
            .bind(id).bind(version).bind(version).fetch_one(&mut *tx).await?;
        if pinned > 0 {
            return Err(runinator_models::errors::WORKSPACE_CONFLICT
                .error("version is referenced by an active workflow or pipeline"));
        }
        let now = Utc::now().timestamp();
        let deleted = sqlx::query(&self.render("UPDATE workspace_snapshots SET deleted_at = ? WHERE workspace_id = ? AND (? IS NULL OR version = ?) AND deleted_at IS NULL"))
            .bind(now).bind(id).bind(version).bind(version).execute(&mut *tx).await?;
        if version.is_some() && deleted.affected() == 0 {
            tx.rollback().await?;
            return Ok(false);
        }
        if version.is_none() {
            sqlx::query(&self.render("UPDATE durable_workspaces SET deleted_at = ? WHERE id = ?"))
                .bind(now)
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(true)
    }
    async fn prune_workspace_leases(&self) -> Result<(), SendableError> {
        sqlx::query(&self.render("DELETE FROM workspace_checkouts WHERE leased_until < ? AND effect_id NOT IN (SELECT e.id FROM workflow_effects e JOIN workflow_runs r ON r.id = e.workflow_run_id WHERE r.finished_at IS NULL)"))
            .bind(Utc::now().timestamp() - 86400).execute(self.pool()).await?;
        sqlx::query(&self.render("DELETE FROM workspace_pins WHERE workflow_run_id NOT IN (SELECT r.id FROM workflow_runs r LEFT JOIN pipeline_runs p ON p.id = r.pipeline_run_id WHERE r.finished_at IS NULL OR (p.id IS NOT NULL AND p.finished_at IS NULL))"))
            .execute(self.pool()).await?;
        Ok(())
    }
    async fn pending_workspace_cleanup(&self) -> Result<Vec<WorkspaceSnapshot>, SendableError> {
        let rows: Vec<String> = sqlx::query_scalar(&self.render("SELECT snapshot_json FROM workspace_snapshots WHERE deleted_at IS NOT NULL AND snapshot_json <> 'null' ORDER BY deleted_at LIMIT 100"))
            .fetch_all(self.pool()).await?;
        rows.into_iter()
            .map(|s| serde_json::from_str(&s).map_err(Into::into))
            .collect()
    }
    async fn finish_workspace_cleanup(&self, id: Uuid, version: i64) -> Result<(), SendableError> {
        sqlx::query(&self.render("UPDATE workspace_snapshots SET snapshot_json = 'null' WHERE workspace_id = ? AND version = ? AND deleted_at IS NOT NULL"))
            .bind(id).bind(version).execute(self.pool()).await?;
        Ok(())
    }
    async fn workspace_references_archive(&self, uri: String) -> Result<bool, SendableError> {
        let count: i64 = sqlx::query_scalar(&self.render("SELECT COUNT(*) FROM workspace_snapshots WHERE archive_uri = ? AND snapshot_json <> 'null'"))
            .bind(uri).fetch_one(self.pool()).await?;
        Ok(count > 0)
    }

    async fn workspace_version_for_run(
        &self,
        id: Uuid,
        run_id: Uuid,
    ) -> Result<Option<i64>, SendableError> {
        Ok(sqlx::query_scalar(&self.render("SELECT s.version FROM workspace_snapshots s JOIN workflow_runs producer ON producer.id = s.workflow_run_id JOIN workflow_runs consumer ON consumer.id = ? WHERE s.workspace_id = ? AND s.deleted_at IS NULL AND (s.workflow_run_id = consumer.id OR (consumer.pipeline_run_id IS NOT NULL AND producer.pipeline_run_id = consumer.pipeline_run_id)) ORDER BY CASE WHEN s.workflow_run_id = consumer.id THEN 0 ELSE 1 END, s.version DESC LIMIT 1"))
            .bind(run_id).bind(id).fetch_optional(self.pool()).await?)
    }
}

impl<B> SqlStore<B>
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
    pub(super) async fn pin_workspace_version(
        &self,
        tx: &mut sqlx::Transaction<'_, B::Db>,
        id: Uuid,
        version: i64,
        run_id: Uuid,
    ) -> Result<(), SendableError> {
        let sql = if self.dialect() == SqlDialect::MariaDb {
            "INSERT INTO workspace_pins (workspace_id, version, workflow_run_id) VALUES (?, ?, ?) ON DUPLICATE KEY UPDATE version = version"
        } else {
            "INSERT INTO workspace_pins (workspace_id, version, workflow_run_id) VALUES (?, ?, ?) ON CONFLICT (workspace_id, version, workflow_run_id) DO NOTHING"
        };
        sqlx::query(&self.render(sql))
            .bind(id)
            .bind(version)
            .bind(run_id)
            .execute(&mut **tx)
            .await?;
        Ok(())
    }
    pub(super) async fn pin_workspace_inputs(
        &self,
        tx: &mut sqlx::Transaction<'_, B::Db>,
        run_id: Uuid,
        org_id: Option<Uuid>,
        value: &Value,
    ) -> Result<(), SendableError> {
        let mut references = Vec::new();
        collect_references(value, &mut references);
        references.sort_by(|a, b| (&a.key, a.version).cmp(&(&b.key, b.version)));
        references.dedup();
        for reference in references {
            let Some(version) = reference.version.filter(|version| *version > 0) else {
                continue;
            };
            let id: Option<Uuid> = sqlx::query_scalar(&self.render("SELECT id FROM durable_workspaces WHERE tenant_key = ? AND workspace_key = ? AND deleted_at IS NULL"))
                .bind(org_id.map(|id| id.to_string()).unwrap_or_default()).bind(reference.key.as_str()).fetch_optional(&mut **tx).await?;
            let Some(id) = id else {
                return Err(runinator_models::errors::WORKSPACE_INVALID
                    .error("pinned workspace does not exist"));
            };
            let locked = sqlx::query(&self.render("UPDATE durable_workspaces SET revision = revision + 1 WHERE id = ? AND deleted_at IS NULL"))
                .bind(id).execute(&mut **tx).await?;
            let exists: Option<i64> = sqlx::query_scalar(&self.render("SELECT version FROM workspace_snapshots WHERE workspace_id = ? AND version = ? AND deleted_at IS NULL"))
                .bind(id).bind(version).fetch_optional(&mut **tx).await?;
            if locked.affected() == 0 || exists.is_none() {
                return Err(runinator_models::errors::WORKSPACE_INVALID
                    .error("pinned workspace version was deleted"));
            }
            self.pin_workspace_version(tx, id, version, run_id).await?;
        }
        Ok(())
    }
}

fn collect_references(value: &Value, references: &mut Vec<WorkspaceReference>) {
    match value {
        Value::Object(object) => {
            for (name, child) in object {
                if matches!(
                    name.as_str(),
                    "workspace" | "workspace_affinity" | "$workspace_default"
                ) {
                    if let Ok(reference) = child.decode::<WorkspaceReference>() {
                        references.push(reference);
                    }
                } else if name == "workspaces"
                    && let Value::Object(items) = child
                {
                    for reference in items.values() {
                        if let Ok(reference) = reference.decode::<WorkspaceReference>() {
                            references.push(reference);
                        }
                    }
                }
                collect_references(child, references);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_references(item, references);
            }
        }
        _ => {}
    }
}
