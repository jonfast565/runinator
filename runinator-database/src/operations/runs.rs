//! run and node-run execution state, plus the ready-node queue.
//!
//! the `RunStore` half of the generic sql implementation. bodies are written once, over any
//! `SqlBackend`; see `super` for the shared helpers they call.

use super::*;

// the bound list is repeated verbatim in every role impl in this directory. it stays spelled out
// rather than hidden behind a macro so that type errors inside the query bodies — the part that
// actually gets edited — keep pointing at real source lines instead of a macro expansion.
impl<B> RunStore for SqlStore<B>
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
    async fn fetch_workflow_runs_by_status(
        &self,
        status: WorkflowStatus,
    ) -> Result<Vec<WorkflowRun>, SendableError> {
        let rows = sqlx::query(&self.render(&format!(
            "SELECT {WORKFLOW_RUN_COLUMNS} FROM workflow_runs WHERE status = ? ORDER BY created_at, id"
        )))
        .bind(status.as_str())
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_workflow_run).collect())
    }

    async fn claim_workflow_runs_for_scheduler(
        &self,
        scheduler_id: String,
        statuses: Vec<WorkflowStatus>,
        now: DateTime<Utc>,
        lease_until: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<WorkflowRun>, SendableError> {
        if statuses.is_empty() {
            return Ok(Vec::new());
        }
        let statuses = status_list(&statuses);
        // mysql has no UPDATE ... RETURNING and forbids a subquery on the table being updated, so
        // claim with a derived-table subselect, then read the rows back by the lease just written.
        if self.dialect() == SqlDialect::MariaDb {
            let claim_sql = self.render(&format!(
                "UPDATE workflow_runs SET scheduler_claimed_by = ?, scheduler_claimed_until = ?
                 WHERE id IN (
                     SELECT id FROM (
                         SELECT id FROM workflow_runs
                         WHERE status IN ({statuses})
                           AND (scheduler_claimed_until IS NULL OR scheduler_claimed_until <= ? OR scheduler_claimed_by = ?)
                         ORDER BY created_at, id
                         LIMIT ?
                     ) AS claimable
                 )",
            ));
            sqlx::query(&claim_sql)
                .bind(scheduler_id.as_str())
                .bind(lease_until.timestamp())
                .bind(now.timestamp())
                .bind(scheduler_id.as_str())
                .bind(limit.max(1))
                .execute(self.pool())
                .await?;
            let rows = sqlx::query(&self.render(&format!(
                "SELECT {WORKFLOW_RUN_COLUMNS} FROM workflow_runs WHERE scheduler_claimed_by = ? AND scheduler_claimed_until = ? ORDER BY created_at, id",
            )))
            .bind(scheduler_id.as_str())
            .bind(lease_until.timestamp())
            .fetch_all(self.pool())
            .await?;
            return Ok(rows.iter().map(mappers::row_to_workflow_run).collect());
        }

        let sql = self.render(&format!(
            "UPDATE workflow_runs SET scheduler_claimed_by = ?, scheduler_claimed_until = ?
             WHERE id IN (
                 SELECT id FROM workflow_runs
                 WHERE status IN ({statuses})
                   AND (scheduler_claimed_until IS NULL OR scheduler_claimed_until <= ? OR scheduler_claimed_by = ?)
                 ORDER BY created_at, id
                 LIMIT ?{skip}
             )
             RETURNING {WORKFLOW_RUN_COLUMNS}",
            skip = self.dialect().skip_locked(),
        ));
        let rows = sqlx::query(&sql)
            .bind(scheduler_id.as_str())
            .bind(lease_until.timestamp())
            .bind(now.timestamp())
            .bind(scheduler_id.as_str())
            .bind(limit.max(1))
            .fetch_all(self.pool())
            .await?;
        Ok(rows.iter().map(mappers::row_to_workflow_run).collect())
    }

    async fn renew_workflow_run_claim(
        &self,
        workflow_run_id: Uuid,
        scheduler_id: String,
        lease_until: DateTime<Utc>,
    ) -> Result<bool, SendableError> {
        let result = sqlx::query(&self.render(
            "UPDATE workflow_runs SET scheduler_claimed_until = ? WHERE id = ? AND scheduler_claimed_by = ?",
        ))
        .bind(lease_until.timestamp())
        .bind(workflow_run_id)
        .bind(scheduler_id)
        .execute(self.pool())
        .await?;
        Ok(result.affected() > 0)
    }

    async fn release_workflow_run_claim(
        &self,
        workflow_run_id: Uuid,
        scheduler_id: String,
    ) -> Result<(), SendableError> {
        self.pool()
            .execute(
                sqlx::query(&self.render(
                    "UPDATE workflow_runs SET scheduler_claimed_by = NULL, scheduler_claimed_until = NULL WHERE id = ? AND scheduler_claimed_by = ?",
                ))
                .bind(workflow_run_id)
                .bind(scheduler_id),
            )
            .await?;
        Ok(())
    }

    async fn fetch_recent_workflow_runs(
        &self,
        limit: i64,
    ) -> Result<Vec<WorkflowRun>, SendableError> {
        let rows = sqlx::query(&self.render(&format!(
            "SELECT {WORKFLOW_RUN_COLUMNS} FROM workflow_runs ORDER BY created_at DESC, id DESC LIMIT ?"
        )))
        .bind(limit)
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_workflow_run).collect())
    }

    async fn delete_workflow_run(&self, workflow_run_id: Uuid) -> Result<(), SendableError> {
        retry_delete(|| async {
            let mut tx = self.pool().begin().await?;
            // deepest dependant first. the cascades declared on the vm tables are not relied on:
            // mysql 8 silently discards a column-level `REFERENCES`, so one engine would cascade
            // and another would orphan.
            for sql in [
                "DELETE FROM workflow_effect_dispatches WHERE effect_id IN (SELECT e.id FROM workflow_effects e JOIN workflow_continuations c ON c.id = e.continuation_id WHERE c.workflow_run_id = ?)",
                "DELETE FROM workflow_effect_output_events WHERE effect_id IN (SELECT e.id FROM workflow_effects e JOIN workflow_continuations c ON c.id = e.continuation_id WHERE c.workflow_run_id = ?)",
                "DELETE FROM workflow_journal_entries WHERE workflow_run_id = ?",
                "DELETE FROM workflow_effects WHERE continuation_id IN (SELECT id FROM workflow_continuations WHERE workflow_run_id = ?)",
                "DELETE FROM workflow_continuations WHERE workflow_run_id = ?",
                "DELETE FROM workflow_vm_modules WHERE workflow_run_id = ?",
                "DELETE FROM workflow_cursor_frames WHERE workflow_run_id = ?",
                "DELETE FROM workflow_run_cursors WHERE workflow_run_id = ?",
                "DELETE FROM workflow_run_frames WHERE workflow_run_id = ?",
                "DELETE FROM workflow_run_pending_interrupts WHERE workflow_run_id = ?",
                "DELETE FROM workflow_run_event_sources WHERE workflow_run_id = ?",
                "DELETE FROM workflow_trigger_firings WHERE workflow_run_id = ?",
                "DELETE FROM pipeline_member_attempts WHERE workflow_run_id = ?",
                "DELETE FROM automation_records WHERE workflow_run_id = ?",
                "DELETE FROM notifications WHERE workflow_run_id = ?",
                "DELETE FROM gates WHERE workflow_run_id = ?",
                "DELETE FROM workflow_runs WHERE id = ?",
            ] {
                sqlx::query(&self.render(sql)).bind(workflow_run_id).execute(&mut *tx).await?;
            }
            tx.commit().await
        }).await?;
        Ok(())
    }

    async fn fetch_open_workflow_runs_created_before(
        &self,
        cutoff: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<WorkflowRun>, SendableError> {
        let columns = WORKFLOW_RUN_COLUMNS;
        let terminal = WorkflowStatus::TERMINAL
            .iter()
            .map(|status| format!("'{}'", status.as_str()))
            .collect::<Vec<_>>()
            .join(", ");
        let rows = sqlx::query(&self.render(&format!(
            "SELECT {columns} FROM workflow_runs
             WHERE status NOT IN ({terminal}) AND created_at < ?
             ORDER BY created_at LIMIT ?",
        )))
        .bind(cutoff.timestamp())
        .bind(limit.clamp(1, 1000))
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_workflow_run).collect())
    }
}
