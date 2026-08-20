//! the standalone task-run model.
//!
//! the `TaskRunStore` half of the generic sql implementation. bodies are written once, over any
//! `SqlBackend`; see `super` for the shared helpers they call.

use super::*;

// the bound list is repeated verbatim in every role impl in this directory. it stays spelled out
// rather than hidden behind a macro so that type errors inside the query bodies — the part that
// actually gets edited — keep pointing at real source lines instead of a macro expansion.
impl<B> TaskRunStore for SqlStore<B>
where
    B: SqlBackend,
    // encode bounds for every bound value type.
    for<'q> i64: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> bool: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> &'q str: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> String: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Vec<u8>: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Uuid: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Option<i64>: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Option<String>: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Option<Uuid>: Encode<'q, B::Db> + Type<B::Db>,
    // decode bounds (operations read a couple of columns directly; mappers read the rest).
    for<'r> i64: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> String: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> bool: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Uuid: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Option<i64>: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Option<String>: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Option<Uuid>: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Vec<u8>: Decode<'r, B::Db> + Type<B::Db>,
    // row indexing + executor plumbing.
    usize: ColumnIndex<<B::Db as Database>::Row>,
    for<'c> &'c str: ColumnIndex<<B::Db as Database>::Row>,
    for<'q> <B::Db as Database>::Arguments<'q>: IntoArguments<'q, B::Db>,
    for<'c> &'c mut <B::Db as Database>::Connection: Executor<'c, Database = B::Db>,
    <B::Db as Database>::QueryResult: RowsAffected,
{
    async fn create_workflow_task_run(
        &self,
        workflow_run_id: Uuid,
        launch_node_run_id: Uuid,
        node_id: String,
        action: WorkflowAction,
        parameters: Value,
    ) -> Result<WorkflowTaskRun, SendableError> {
        let id = Uuid::now_v7();
        let now = Utc::now().timestamp();
        sqlx::query(&self.render(
            "INSERT INTO workflow_task_runs (id, workflow_run_id, launch_node_run_id, node_id, action, status, attempt, parameters, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        ))
        .bind(id).bind(workflow_run_id).bind(launch_node_run_id).bind(node_id)
        .bind(serde_json::to_string(&action)?).bind(WorkflowStatus::Queued.as_str())
        .bind(0i64).bind(parameters.to_string()).bind(now).execute(self.pool()).await?;
        self.fetch_workflow_task_run(id)
            .await?
            .ok_or_else(|| "created task run disappeared".into())
    }

    async fn fetch_workflow_task_run(
        &self,
        task_run_id: Uuid,
    ) -> Result<Option<WorkflowTaskRun>, SendableError> {
        let row = sqlx::query(&self.render(
            "SELECT id, workflow_run_id, launch_node_run_id, node_id, action, status, attempt, parameters, output_json, message, current_executor_replica_id, last_executor_replica_id, executor_claimed_at, executor_released_at, created_at, started_at, finished_at FROM workflow_task_runs WHERE id = ?",
        )).bind(task_run_id).fetch_optional(self.pool()).await?;
        Ok(row.as_ref().map(mappers::row_to_workflow_task_run))
    }

    async fn fetch_workflow_task_runs(
        &self,
        workflow_run_id: Uuid,
    ) -> Result<Vec<WorkflowTaskRun>, SendableError> {
        let rows = sqlx::query(&self.render(
            "SELECT id, workflow_run_id, launch_node_run_id, node_id, action, status, attempt, parameters, output_json, message, current_executor_replica_id, last_executor_replica_id, executor_claimed_at, executor_released_at, created_at, started_at, finished_at FROM workflow_task_runs WHERE workflow_run_id = ? ORDER BY created_at, id",
        ))
        .bind(workflow_run_id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_workflow_task_run).collect())
    }

    async fn update_workflow_task_run(
        &self,
        task_run_id: Uuid,
        status: WorkflowStatus,
        attempt: Option<i64>,
        output_json: Option<Value>,
        message: Option<String>,
    ) -> Result<(), SendableError> {
        let now = Utc::now().timestamp();
        let terminal = status.is_terminal();
        sqlx::query(&self.render(
            "UPDATE workflow_task_runs SET status = ?, attempt = COALESCE(?, attempt), output_json = COALESCE(?, output_json), message = COALESCE(?, message), started_at = CASE WHEN ? = 'running' AND started_at IS NULL THEN ? ELSE started_at END, finished_at = CASE WHEN ? THEN ? ELSE finished_at END WHERE id = ?",
        ))
        .bind(status.as_str()).bind(attempt).bind(output_json.map(|value| value.to_string())).bind(message)
        .bind(status.as_str()).bind(now).bind(terminal).bind(now).bind(task_run_id)
        .execute(self.pool()).await?;
        Ok(())
    }

    async fn fetch_runs_by_status(
        &self,
        status: RunStatus,
    ) -> Result<Vec<RunSummary>, SendableError> {
        let sql = self.render(&format!(
            "SELECT id, status, parameters, output_json, message, {trigger}, started_at, finished_at, created_at, workflow_run_id, workflow_node_id FROM runs WHERE status = ? ORDER BY created_at, id",
            trigger = self.dialect().ident("trigger"),
        ));
        let rows = sqlx::query(&sql)
            .bind(status.as_str())
            .fetch_all(self.pool())
            .await?;
        Ok(rows.iter().map(mappers::row_to_run_summary).collect())
    }

    async fn update_run_status(
        &self,
        run_id: Uuid,
        status: RunStatus,
        output_json: Option<Value>,
        message: Option<String>,
    ) -> Result<(), SendableError> {
        let now = Utc::now().timestamp();
        let terminal = matches!(
            status,
            RunStatus::Succeeded | RunStatus::Failed | RunStatus::TimedOut | RunStatus::Canceled
        );
        self.pool()
            .execute(
                sqlx::query(&self.render(
                    "UPDATE runs SET status = ?, output_json = COALESCE(?, output_json), message = COALESCE(?, message), started_at = CASE WHEN ? = 'running' AND started_at IS NULL THEN ? ELSE started_at END, finished_at = CASE WHEN ? THEN ? ELSE finished_at END WHERE id = ?",
                ))
                .bind(status.as_str())
                .bind(output_json.map(|v| v.to_string()))
                .bind(message)
                .bind(status.as_str())
                .bind(now)
                .bind(terminal)
                .bind(now)
                .bind(run_id),
            )
            .await?;
        Ok(())
    }

    async fn append_run_chunk(
        &self,
        run_id: Uuid,
        chunk: &NewRunChunk,
    ) -> Result<RunChunk, SendableError> {
        let sequence: i64 = sqlx::query(&self.render(
            "SELECT COALESCE(MAX(sequence), 0) + 1 AS next_sequence FROM run_chunks WHERE run_id = ?",
        ))
        .bind(run_id)
        .fetch_one(self.pool())
        .await?
        .get("next_sequence");
        let columns = "id, run_id, sequence, stream, content, created_at";
        let id = Uuid::now_v7();
        let created_at = Utc::now().timestamp();
        if self.dialect() == SqlDialect::MySql {
            let mut conn = self.pool().acquire().await?;
            sqlx::query(&self.render(
                "INSERT INTO run_chunks (id, run_id, sequence, stream, content, created_at) VALUES (?, ?, ?, ?, ?, ?)",
            ))
            .bind(id)
            .bind(run_id)
            .bind(sequence)
            .bind(chunk.stream.as_str())
            .bind(chunk.content.as_str())
            .bind(created_at)
            .execute(&mut *conn)
            .await?;
            let row = sqlx::query(
                &self.render(&format!("SELECT {columns} FROM run_chunks WHERE id = ?")),
            )
            .bind(id)
            .fetch_one(&mut *conn)
            .await?;
            return Ok(mappers::row_to_run_chunk(&row));
        }
        let row = sqlx::query(&self.render(&format!(
            "INSERT INTO run_chunks (id, run_id, sequence, stream, content, created_at)
             VALUES (?, ?, ?, ?, ?, ?)
             RETURNING {columns}",
        )))
        .bind(id)
        .bind(run_id)
        .bind(sequence)
        .bind(chunk.stream.as_str())
        .bind(chunk.content.as_str())
        .bind(created_at)
        .fetch_one(self.pool())
        .await?;
        Ok(mappers::row_to_run_chunk(&row))
    }

    async fn fetch_run_chunks(
        &self,
        run_id: Uuid,
        cursor: Option<i64>,
        limit: i64,
    ) -> Result<Vec<RunChunk>, SendableError> {
        let rows = sqlx::query(&self.render(
            "SELECT id, run_id, sequence, stream, content, created_at FROM run_chunks WHERE run_id = ? AND sequence > ? ORDER BY sequence ASC LIMIT ?",
        ))
        .bind(run_id)
        .bind(cursor.unwrap_or(0))
        .bind(limit.clamp(1, 1000))
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_run_chunk).collect())
    }

    async fn add_run_artifact(
        &self,
        run_id: Uuid,
        artifact: &NewRunArtifact,
    ) -> Result<RunArtifact, SendableError> {
        let columns = "id, run_id, name, mime_type, size_bytes, uri, metadata, created_at";
        let id = Uuid::now_v7();
        let created_at = Utc::now().timestamp();
        if self.dialect() == SqlDialect::MySql {
            let mut conn = self.pool().acquire().await?;
            sqlx::query(&self.render(
                "INSERT INTO run_artifacts (id, run_id, name, mime_type, size_bytes, uri, metadata, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            ))
            .bind(id)
            .bind(run_id)
            .bind(artifact.name.as_str())
            .bind(artifact.mime_type.as_str())
            .bind(artifact.size_bytes)
            .bind(artifact.uri.as_str())
            .bind(artifact.metadata.to_string())
            .bind(created_at)
            .execute(&mut *conn)
            .await?;
            let row = sqlx::query(
                &self.render(&format!("SELECT {columns} FROM run_artifacts WHERE id = ?")),
            )
            .bind(id)
            .fetch_one(&mut *conn)
            .await?;
            return Ok(mappers::row_to_run_artifact(&row));
        }
        let row = sqlx::query(&self.render(&format!(
            "INSERT INTO run_artifacts (id, run_id, name, mime_type, size_bytes, uri, metadata, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             RETURNING {columns}",
        )))
        .bind(id)
        .bind(run_id)
        .bind(artifact.name.as_str())
        .bind(artifact.mime_type.as_str())
        .bind(artifact.size_bytes)
        .bind(artifact.uri.as_str())
        .bind(artifact.metadata.to_string())
        .bind(created_at)
        .fetch_one(self.pool())
        .await?;
        Ok(mappers::row_to_run_artifact(&row))
    }

    async fn fetch_run_artifacts(&self, run_id: Uuid) -> Result<Vec<RunArtifact>, SendableError> {
        let rows = sqlx::query(&self.render(
            "SELECT id, run_id, name, mime_type, size_bytes, uri, metadata, created_at FROM run_artifacts WHERE run_id = ? ORDER BY created_at ASC, id ASC",
        ))
        .bind(run_id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_run_artifact).collect())
    }

    async fn fetch_all_artifacts(&self) -> Result<Vec<RunArtifact>, SendableError> {
        let rows = sqlx::query(
            "SELECT id, run_id, name, mime_type, size_bytes, uri, metadata, created_at FROM run_artifacts ORDER BY created_at DESC, id DESC",
        )
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_run_artifact).collect())
    }

    async fn fetch_artifact(
        &self,
        artifact_id: Uuid,
    ) -> Result<Option<RunArtifact>, SendableError> {
        let row = sqlx::query(&self.render(
            "SELECT id, run_id, name, mime_type, size_bytes, uri, metadata, created_at FROM run_artifacts WHERE id = ?",
        ))
        .bind(artifact_id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|row| mappers::row_to_run_artifact(&row)))
    }

    async fn delete_artifact(&self, artifact_id: Uuid) -> Result<bool, SendableError> {
        Ok(retry_delete(|| async {
            sqlx::query(&self.render("DELETE FROM run_artifacts WHERE id = ?"))
                .bind(artifact_id)
                .execute(self.pool())
                .await
                .map(|result| result.affected() > 0)
        })
        .await?)
    }
}
