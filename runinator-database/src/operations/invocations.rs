//! resumable invocations and the durable calls they yield on.
//!
//! the `InvocationStore` half of the generic sql implementation. bodies are written once, over any
//! `SqlBackend`; see `super` for the shared helpers they call.
//!
//! two operations here are transactional on purpose. `suspend_invocation` writes the continuation,
//! the call row and the outbox row together, because any two of those three without the third is a
//! wedged run: a continuation with no call waits forever, a call with no continuation resumes a
//! program that never suspended, and a call with no outbox row is a dispatch nobody makes.
//! `retry_invocation_call` has the same shape for the same reason.

use super::*;

/// every column `mappers::row_to_invocation` reads.
const INVOCATION_COLUMNS: &str = "id, workflow_run_id, workflow_node_run_id, cursor_id, node_id, \
                                  module_version, continuation, status, output_json, message, \
                                  created_at, updated_at, finished_at";

/// every column `mappers::row_to_invocation_call` reads.
const INVOCATION_CALL_COLUMNS: &str = "id, invocation_id, workflow_run_id, sequence, target, arguments, policy, attempt, status, \
     result_json, message, idempotency_key, deadline_at, current_executor_replica_id, \
     last_executor_replica_id, executor_claimed_at, executor_released_at, created_at, started_at, \
     finished_at";

impl<B> InvocationStore for SqlStore<B>
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
    async fn create_invocation(
        &self,
        workflow_run_id: Uuid,
        workflow_node_run_id: Uuid,
        cursor_id: Option<Uuid>,
        node_id: &str,
        module_version: u32,
        continuation: &InvocationContinuation,
    ) -> Result<WorkflowInvocation, SendableError> {
        let id = Uuid::now_v7();
        let now = Utc::now().timestamp();
        sqlx::query(&self.render(&format!(
            "INSERT INTO workflow_invocations ({INVOCATION_COLUMNS}) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, NULL, NULL, ?, ?, NULL)"
        )))
        .bind(id)
        .bind(workflow_run_id)
        .bind(workflow_node_run_id)
        .bind(cursor_id)
        .bind(node_id)
        .bind(module_version as i64)
        .bind(serde_json::to_string(continuation)?)
        .bind(WorkflowStatus::Running.as_str())
        .bind(now)
        .bind(now)
        .execute(self.pool())
        .await?;
        self.fetch_invocation(id)
            .await?
            .ok_or_else(|| -> SendableError { "invocation vanished after insert".into() })
    }

    async fn fetch_invocation(
        &self,
        invocation_id: Uuid,
    ) -> Result<Option<WorkflowInvocation>, SendableError> {
        let row = sqlx::query(&self.render(&format!(
            "SELECT {INVOCATION_COLUMNS} FROM workflow_invocations WHERE id = ?"
        )))
        .bind(invocation_id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|row| mappers::row_to_invocation(&row)))
    }

    async fn fetch_invocation_for_node_run(
        &self,
        workflow_node_run_id: Uuid,
    ) -> Result<Option<WorkflowInvocation>, SendableError> {
        let row = sqlx::query(&self.render(&format!(
            "SELECT {INVOCATION_COLUMNS} FROM workflow_invocations \
             WHERE workflow_node_run_id = ? ORDER BY created_at DESC"
        )))
        .bind(workflow_node_run_id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|row| mappers::row_to_invocation(&row)))
    }

    async fn fetch_invocations_for_run(
        &self,
        workflow_run_id: Uuid,
    ) -> Result<Vec<WorkflowInvocation>, SendableError> {
        let rows = sqlx::query(&self.render(&format!(
            "SELECT {INVOCATION_COLUMNS} FROM workflow_invocations \
             WHERE workflow_run_id = ? ORDER BY created_at ASC"
        )))
        .bind(workflow_run_id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_invocation).collect())
    }

    async fn suspend_invocation(
        &self,
        continuation: &InvocationContinuation,
        call: NewInvocationCall,
        command: ActionCommand,
    ) -> Result<WorkflowInvocationCall, SendableError> {
        // an already-recorded sequence means this drive is a duplicate: the program was re-stepped
        // and reached the same call again. return what is already there and enqueue nothing.
        if let Some(existing) = self
            .call_at_sequence(call.invocation_id, call.sequence)
            .await?
        {
            return Ok(existing);
        }

        let now = Utc::now().timestamp();
        let id = call.id;
        let mut tx = self.pool().begin().await?;

        sqlx::query(&self.render(
            "UPDATE workflow_invocations SET continuation = ?, status = ?, updated_at = ? WHERE id = ?",
        ))
        .bind(serde_json::to_string(continuation)?)
        .bind(WorkflowStatus::Running.as_str())
        .bind(now)
        .bind(call.invocation_id)
        .execute(&mut *tx)
        .await?;

        sqlx::query(&self.render(&format!(
            "INSERT INTO workflow_invocation_calls ({INVOCATION_CALL_COLUMNS}) \
             VALUES (?, ?, ?, ?, ?, ?, ?, 0, ?, NULL, NULL, ?, ?, NULL, NULL, NULL, NULL, ?, ?, NULL)"
        )))
        .bind(id)
        .bind(call.invocation_id)
        .bind(call.workflow_run_id)
        .bind(call.sequence)
        .bind(serde_json::to_string(&call.target)?)
        .bind(serde_json::to_string(&call.arguments)?)
        .bind(serde_json::to_string(&call.policy)?)
        .bind(WorkflowStatus::Running.as_str())
        .bind(call.idempotency_key.clone())
        .bind(call.deadline_at)
        .bind(now)
        .bind(now)
        .execute(&mut *tx)
        .await?;

        let dedupe_key = format!("workflow-invocation-call:{id}:0");
        self.enqueue_dispatch_in(&mut tx, &dedupe_key, &command, now)
            .await?;

        tx.commit().await?;
        self.fetch_invocation_call(id)
            .await?
            .ok_or_else(|| -> SendableError { "invocation call vanished after insert".into() })
    }

    async fn update_invocation_continuation(
        &self,
        invocation_id: Uuid,
        continuation: &InvocationContinuation,
    ) -> Result<(), SendableError> {
        sqlx::query(&self.render(
            "UPDATE workflow_invocations SET continuation = ?, updated_at = ? WHERE id = ?",
        ))
        .bind(serde_json::to_string(continuation)?)
        .bind(Utc::now().timestamp())
        .bind(invocation_id)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    async fn settle_invocation(
        &self,
        invocation_id: Uuid,
        status: WorkflowStatus,
        output: Option<Value>,
        message: Option<String>,
    ) -> Result<(), SendableError> {
        let now = Utc::now().timestamp();
        let finished = status.is_terminal().then_some(now);
        sqlx::query(&self.render(
            "UPDATE workflow_invocations \
             SET status = ?, output_json = ?, message = ?, updated_at = ?, finished_at = ? \
             WHERE id = ?",
        ))
        .bind(status.as_str())
        .bind(output.map(|value| value.to_string()))
        .bind(message)
        .bind(now)
        .bind(finished)
        .bind(invocation_id)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    async fn fetch_invocation_call(
        &self,
        call_id: Uuid,
    ) -> Result<Option<WorkflowInvocationCall>, SendableError> {
        let row = sqlx::query(&self.render(&format!(
            "SELECT {INVOCATION_CALL_COLUMNS} FROM workflow_invocation_calls WHERE id = ?"
        )))
        .bind(call_id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|row| mappers::row_to_invocation_call(&row)))
    }

    async fn fetch_invocation_calls(
        &self,
        invocation_id: Uuid,
    ) -> Result<Vec<WorkflowInvocationCall>, SendableError> {
        let rows = sqlx::query(&self.render(&format!(
            "SELECT {INVOCATION_CALL_COLUMNS} FROM workflow_invocation_calls \
             WHERE invocation_id = ? ORDER BY sequence ASC"
        )))
        .bind(invocation_id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_invocation_call).collect())
    }

    async fn fetch_pending_invocation_call(
        &self,
        invocation_id: Uuid,
    ) -> Result<Option<WorkflowInvocationCall>, SendableError> {
        let statuses = status_list(&[WorkflowStatus::Queued, WorkflowStatus::Running]);
        let row = sqlx::query(&self.render(&format!(
            "SELECT {INVOCATION_CALL_COLUMNS} FROM workflow_invocation_calls \
             WHERE invocation_id = ? AND status IN ({statuses}) ORDER BY sequence DESC"
        )))
        .bind(invocation_id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|row| mappers::row_to_invocation_call(&row)))
    }

    async fn settle_invocation_call(
        &self,
        call_id: Uuid,
        attempt: i64,
        status: WorkflowStatus,
        result: Option<Value>,
        message: Option<String>,
    ) -> Result<bool, SendableError> {
        let now = Utc::now().timestamp();
        let open = status_list(&[WorkflowStatus::Queued, WorkflowStatus::Running]);
        // guarded on both the attempt and the open statuses: a result from a superseded attempt, or
        // a duplicate of one already applied, must not overwrite the settled outcome.
        let affected = sqlx::query(&self.render(&format!(
            "UPDATE workflow_invocation_calls \
             SET status = ?, result_json = ?, message = ?, finished_at = ?, executor_released_at = ? \
             WHERE id = ? AND attempt = ? AND status IN ({open})"
        )))
        .bind(status.as_str())
        .bind(result.map(|value| value.to_string()))
        .bind(message)
        .bind(now)
        .bind(now)
        .bind(call_id)
        .bind(attempt)
        .execute(self.pool())
        .await?
        .affected();
        Ok(affected > 0)
    }

    async fn retry_invocation_call(
        &self,
        call_id: Uuid,
        deadline_at: Option<i64>,
        command: ActionCommand,
    ) -> Result<WorkflowInvocationCall, SendableError> {
        let now = Utc::now().timestamp();
        let mut tx = self.pool().begin().await?;

        sqlx::query(&self.render(
            "UPDATE workflow_invocation_calls \
             SET attempt = attempt + 1, status = ?, result_json = NULL, message = NULL, \
                 deadline_at = ?, started_at = ?, finished_at = NULL, \
                 current_executor_replica_id = NULL, executor_claimed_at = NULL, \
                 executor_released_at = NULL \
             WHERE id = ?",
        ))
        .bind(WorkflowStatus::Running.as_str())
        .bind(deadline_at)
        .bind(now)
        .bind(call_id)
        .execute(&mut *tx)
        .await?;

        let attempt =
            sqlx::query(&self.render("SELECT attempt FROM workflow_invocation_calls WHERE id = ?"))
                .bind(call_id)
                .fetch_one(&mut *tx)
                .await?
                .get::<i64, _>("attempt");

        let dedupe_key = format!("workflow-invocation-call:{call_id}:{attempt}");
        self.enqueue_dispatch_in(&mut tx, &dedupe_key, &command, now)
            .await?;

        tx.commit().await?;
        self.fetch_invocation_call(call_id)
            .await?
            .ok_or_else(|| -> SendableError { "invocation call vanished after retry".into() })
    }

    async fn set_invocation_call_executor(
        &self,
        call_id: Uuid,
        replica_id: Option<Uuid>,
    ) -> Result<(), SendableError> {
        let now = Utc::now().timestamp();
        match replica_id {
            Some(replica_id) => {
                sqlx::query(&self.render(
                    "UPDATE workflow_invocation_calls \
                     SET current_executor_replica_id = ?, last_executor_replica_id = ?, \
                         executor_claimed_at = ?, executor_released_at = NULL \
                     WHERE id = ?",
                ))
                .bind(replica_id)
                .bind(replica_id)
                .bind(now)
                .bind(call_id)
                .execute(self.pool())
                .await?;
            }
            None => {
                sqlx::query(&self.render(
                    "UPDATE workflow_invocation_calls \
                     SET current_executor_replica_id = NULL, executor_released_at = ? WHERE id = ?",
                ))
                .bind(now)
                .bind(call_id)
                .execute(self.pool())
                .await?;
            }
        }
        Ok(())
    }

    async fn cancel_invocation_calls_for_run(
        &self,
        workflow_run_id: Uuid,
        message: &str,
    ) -> Result<u64, SendableError> {
        let now = Utc::now().timestamp();
        let open = status_list(&[WorkflowStatus::Queued, WorkflowStatus::Running]);
        let affected = sqlx::query(&self.render(&format!(
            "UPDATE workflow_invocation_calls \
             SET status = ?, message = ?, finished_at = ?, executor_released_at = ? \
             WHERE workflow_run_id = ? AND status IN ({open})"
        )))
        .bind(WorkflowStatus::Canceled.as_str())
        .bind(message)
        .bind(now)
        .bind(now)
        .bind(workflow_run_id)
        .execute(self.pool())
        .await?
        .affected();
        Ok(affected)
    }
}

impl<B> SqlStore<B>
where
    B: SqlBackend,
    for<'q> i64: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> &'q str: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> String: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Uuid: Encode<'q, B::Db> + Type<B::Db>,
    for<'r> i64: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> String: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> bool: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Uuid: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Option<i64>: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Option<String>: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Option<Uuid>: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Vec<u8>: Decode<'r, B::Db> + Type<B::Db>,
    usize: ColumnIndex<<B::Db as Database>::Row>,
    for<'c> &'c str: ColumnIndex<<B::Db as Database>::Row>,
    for<'q> <B::Db as Database>::Arguments<'q>: IntoArguments<'q, B::Db>,
    for<'c> &'c mut <B::Db as Database>::Connection: Executor<'c, Database = B::Db>,
{
    async fn call_at_sequence(
        &self,
        invocation_id: Uuid,
        sequence: i64,
    ) -> Result<Option<WorkflowInvocationCall>, SendableError> {
        let row = sqlx::query(&self.render(&format!(
            "SELECT {INVOCATION_CALL_COLUMNS} FROM workflow_invocation_calls \
             WHERE invocation_id = ? AND sequence = ?"
        )))
        .bind(invocation_id)
        .bind(sequence)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|row| mappers::row_to_invocation_call(&row)))
    }

    /// the outbox insert, run inside a caller's transaction.
    ///
    /// this duplicates `enqueue_action_dispatch`'s statement rather than calling it because that one
    /// runs against the pool: calling it here would put the outbox row outside the transaction the
    /// call row is in, which is exactly the split this module exists to prevent.
    async fn enqueue_dispatch_in(
        &self,
        tx: &mut sqlx::Transaction<'_, B::Db>,
        dedupe_key: &str,
        command: &ActionCommand,
        now: i64,
    ) -> Result<(), SendableError> {
        let sql = if self.dialect() == SqlDialect::MySql {
            "INSERT INTO workflow_action_dispatches (id, dedupe_key, command_json, attempts, created_at, updated_at)
             VALUES (?, ?, ?, 0, ?, ?)
             ON DUPLICATE KEY UPDATE command_json = command_json"
        } else {
            "INSERT INTO workflow_action_dispatches (id, dedupe_key, command_json, attempts, created_at, updated_at)
             VALUES (?, ?, ?, 0, ?, ?)
             ON CONFLICT(dedupe_key) DO UPDATE SET command_json = workflow_action_dispatches.command_json"
        };
        sqlx::query(&self.render(sql))
            .bind(Uuid::now_v7())
            .bind(dedupe_key)
            .bind(serde_json::to_string(command)?)
            .bind(now)
            .bind(now)
            .execute(&mut **tx)
            .await?;
        Ok(())
    }
}
