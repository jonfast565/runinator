//! Durable admission-scoped worker-local workspace leases.

use super::*;

const WORKSPACE_COLUMNS: &str = "id, admission_id, generation, scope, attempt, worker_instance_id, worker_replica_id, local_key, requirements, status, version, leased_until, unavailable_since, evidence, created_at, updated_at";

impl<B> WorkspaceStore for SqlStore<B>
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
    async fn allocate_workspace(
        &self,
        workspace: NewWorkspaceLease,
    ) -> Result<WorkspaceLease, SendableError> {
        let now = Utc::now().timestamp();
        let sql = if self.dialect() == SqlDialect::MySql {
            "INSERT INTO workspace_leases (id, admission_id, generation, scope, attempt, worker_instance_id, worker_replica_id, local_key, requirements, status, version, leased_until, evidence, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'allocating', 1, ?, 'null', ?, ?)
             ON DUPLICATE KEY UPDATE id = id"
        } else {
            "INSERT INTO workspace_leases (id, admission_id, generation, scope, attempt, worker_instance_id, worker_replica_id, local_key, requirements, status, version, leased_until, evidence, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'allocating', 1, ?, 'null', ?, ?)
             ON CONFLICT(admission_id, generation, scope, attempt) DO NOTHING"
        };
        sqlx::query(&self.render(sql))
            .bind(workspace.id)
            .bind(workspace.admission_id)
            .bind(workspace.generation)
            .bind(workspace.scope.as_str())
            .bind(workspace.attempt)
            .bind(workspace.worker_instance_id.as_str())
            .bind(workspace.worker_replica_id)
            .bind(workspace.local_key.as_str())
            .bind(workspace.requirements.to_string())
            .bind(workspace.leased_until.timestamp())
            .bind(now)
            .bind(now)
            .execute(self.pool())
            .await?;
        self.fetch_workspace_attempt(
            workspace.admission_id,
            workspace.generation,
            workspace.scope,
            workspace.attempt,
        )
        .await?
        .ok_or_else(|| {
            Box::new(std::io::Error::other("allocated workspace disappeared")) as SendableError
        })
    }

    async fn fetch_workspace(
        &self,
        workspace_id: Uuid,
    ) -> Result<Option<WorkspaceLease>, SendableError> {
        let row = sqlx::query(&self.render(&format!(
            "SELECT {WORKSPACE_COLUMNS} FROM workspace_leases WHERE id = ?"
        )))
        .bind(workspace_id)
        .fetch_optional(self.pool())
        .await?;
        row.map(|row| mappers::row_to_workspace_lease(&row))
            .transpose()
    }

    async fn fetch_workspace_attempt(
        &self,
        admission_id: Uuid,
        generation: i64,
        scope: String,
        attempt: i64,
    ) -> Result<Option<WorkspaceLease>, SendableError> {
        let row = sqlx::query(&self.render(&format!(
            "SELECT {WORKSPACE_COLUMNS} FROM workspace_leases WHERE admission_id = ? AND generation = ? AND scope = ? AND attempt = ?"
        )))
        .bind(admission_id)
        .bind(generation)
        .bind(scope)
        .bind(attempt)
        .fetch_optional(self.pool())
        .await?;
        row.map(|row| mappers::row_to_workspace_lease(&row))
            .transpose()
    }

    async fn transition_workspace_cas(
        &self,
        workspace_id: Uuid,
        expected_version: i64,
        expected_status: WorkspaceStatus,
        next_status: WorkspaceStatus,
        evidence: Option<Value>,
        now: DateTime<Utc>,
    ) -> Result<bool, SendableError> {
        let changed = sqlx::query(&self.render(
            "UPDATE workspace_leases SET status = ?, version = version + 1,
             evidence = COALESCE(?, evidence), updated_at = ?
             WHERE id = ? AND version = ? AND status = ?",
        ))
        .bind(next_status.as_str())
        .bind(evidence.map(|value| value.to_string()))
        .bind(now.timestamp())
        .bind(workspace_id)
        .bind(expected_version)
        .bind(expected_status.as_str())
        .execute(self.pool())
        .await?;
        Ok(changed.affected() > 0)
    }

    async fn renew_workspace(
        &self,
        workspace_id: Uuid,
        expected_version: i64,
        worker_instance_id: String,
        worker_replica_id: Option<Uuid>,
        leased_until: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<bool, SendableError> {
        let changed = sqlx::query(&self.render(
            "UPDATE workspace_leases SET worker_replica_id = ?, leased_until = ?, unavailable_since = NULL, updated_at = ?
             WHERE id = ? AND version = ? AND worker_instance_id = ? AND status NOT IN ('released', 'abandoned')",
        ))
        .bind(worker_replica_id)
        .bind(leased_until.timestamp())
        .bind(now.timestamp())
        .bind(workspace_id)
        .bind(expected_version)
        .bind(worker_instance_id)
        .execute(self.pool())
        .await?;
        Ok(changed.affected() > 0)
    }

    async fn mark_workspace_unavailable(
        &self,
        worker_instance_id: String,
        now: DateTime<Utc>,
    ) -> Result<u64, SendableError> {
        let changed = sqlx::query(&self.render(
            "UPDATE workspace_leases SET unavailable_since = COALESCE(unavailable_since, ?), updated_at = ?
             WHERE worker_instance_id = ? AND status NOT IN ('released', 'abandoned')",
        ))
        .bind(now.timestamp())
        .bind(now.timestamp())
        .bind(worker_instance_id)
        .execute(self.pool())
        .await?;
        Ok(changed.affected())
    }

    async fn fetch_expired_workspaces(
        &self,
        now: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<WorkspaceLease>, SendableError> {
        let rows = sqlx::query(&self.render(&format!(
            "SELECT {WORKSPACE_COLUMNS} FROM workspace_leases WHERE status NOT IN ('released', 'abandoned') AND leased_until <= ? ORDER BY leased_until, id LIMIT ?"
        )))
        .bind(now.timestamp())
        .bind(limit.max(1))
        .fetch_all(self.pool())
        .await?;
        rows.iter().map(mappers::row_to_workspace_lease).collect()
    }
}
