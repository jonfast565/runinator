//! the operations the workflow state machine calls.
//!
//! the `RuntimeStore` half of the generic sql implementation. bodies are written once, over any
//! `SqlBackend`; see `super` for the shared helpers they call.

use super::*;

// the bound list is repeated verbatim in every role impl in this directory. it stays spelled out
// rather than hidden behind a macro so that type errors inside the query bodies — the part that
// actually gets edited — keep pointing at real source lines instead of a macro expansion.
impl<B> RuntimeStore for SqlStore<B>
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
    SqlStore<B>: WorkflowVmStore,
{
    async fn bootstrap_workflow_vm_run(
        &self,
        start: NewWorkflowVmRun,
    ) -> Result<WorkflowRun, SendableError> {
        WorkflowVmStore::create_workflow_vm_run(self, start).await
    }

    async fn fetch_workflow_vm_result(
        &self,
        workflow_run_id: Uuid,
    ) -> Result<Option<Value>, SendableError> {
        let journal = WorkflowVmStore::fetch_workflow_journal(self, workflow_run_id).await?;
        Ok(journal
            .into_iter()
            .rev()
            .find_map(|record| match record.entry {
                WorkflowJournalEntry::Completed { value, .. } => Some(value),
                _ => None,
            }))
    }

    async fn fetch_workflow(
        &self,
        workflow_id: Uuid,
    ) -> Result<Option<WorkflowDefinition>, SendableError> {
        let row = sqlx::query(&self.render("SELECT id, name, namespace, org_id, version, enabled, input_schema, definition, created_at, updated_at FROM workflows WHERE id = ?"))
            .bind(workflow_id)
            .fetch_optional(self.pool())
            .await?;
        Ok(row.map(|row| mappers::row_to_workflow(&row)))
    }

    async fn fetch_workflow_triggers(
        &self,
        workflow_id: Uuid,
    ) -> Result<Vec<WorkflowTrigger>, SendableError> {
        let rows = sqlx::query(&self.render("SELECT id, workflow_id, kind, enabled, configuration, next_execution, blackout_start, blackout_end, metadata, created_at, updated_at FROM workflow_triggers WHERE workflow_id = ? ORDER BY created_at, id"))
            .bind(workflow_id)
            .fetch_all(self.pool())
            .await?;
        Ok(rows.iter().map(mappers::row_to_workflow_trigger).collect())
    }

    async fn fetch_pipeline(&self, pipeline_id: Uuid) -> Result<Option<Pipeline>, SendableError> {
        let row = sqlx::query(&self.render(&format!(
            "SELECT {PIPELINE_COLUMNS} FROM pipelines WHERE id = ?"
        )))
        .bind(pipeline_id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|row| mappers::row_to_pipeline(&row)))
    }

    async fn fetch_enabled_chained_pipeline_triggers(
        &self,
    ) -> Result<Vec<PipelineTrigger>, SendableError> {
        let sql = self.render(&format!(
            "SELECT {PIPELINE_TRIGGER_COLUMNS} FROM pipeline_triggers WHERE enabled = {} AND kind = 'chained' ORDER BY created_at, id",
            self.dialect().bool_true(),
        ));
        let rows = sqlx::query(&sql).fetch_all(self.pool()).await?;
        Ok(rows.iter().map(mappers::row_to_pipeline_trigger).collect())
    }

    async fn fetch_pipeline_run(
        &self,
        pipeline_run_id: Uuid,
    ) -> Result<Option<PipelineRun>, SendableError> {
        let row = sqlx::query(&self.render(&format!(
            "SELECT {PIPELINE_RUN_COLUMNS} FROM pipeline_runs WHERE id = ?"
        )))
        .bind(pipeline_run_id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|row| mappers::row_to_pipeline_run(&row)))
    }

    async fn fetch_pipeline_runs_for_concurrency(
        &self,
        pipeline_id: Uuid,
    ) -> Result<Vec<PipelineRun>, SendableError> {
        let rows = sqlx::query(&self.render(&format!(
            "SELECT {PIPELINE_RUN_COLUMNS} FROM pipeline_runs WHERE pipeline_id = ? ORDER BY created_at, id"
        ))).bind(pipeline_id).fetch_all(self.pool()).await?;
        Ok(rows.iter().map(mappers::row_to_pipeline_run).collect())
    }

    async fn update_pipeline_run_status(
        &self,
        pipeline_run_id: Uuid,
        status: WorkflowStatus,
        state: Option<Value>,
        message: Option<String>,
    ) -> Result<(), SendableError> {
        let now = Utc::now().timestamp();
        let terminal = status.is_terminal();
        self.pool()
            .execute(
                sqlx::query(&self.render(
                    "UPDATE pipeline_runs SET status = ?, state = COALESCE(?, state), message = COALESCE(?, message), started_at = CASE WHEN ? = 'running' AND started_at IS NULL THEN ? ELSE started_at END, finished_at = CASE WHEN ? THEN ? ELSE finished_at END WHERE id = ?",
                ))
                .bind(status.as_str())
                .bind(state.map(|value| value.to_string()))
                .bind(message)
                .bind(status.as_str())
                .bind(now)
                .bind(terminal)
                .bind(now)
                .bind(pipeline_run_id),
            )
            .await?;
        Ok(())
    }

    async fn reopen_pipeline_run(
        &self,
        pipeline_run_id: Uuid,
        message: String,
    ) -> Result<(), SendableError> {
        sqlx::query(&self.render(
            "UPDATE pipeline_runs SET status = 'running', finished_at = NULL, message = ? WHERE id = ? AND status IN ('failed', 'timed_out')",
        ))
        .bind(message)
        .bind(pipeline_run_id)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    async fn set_workflow_run_pipeline_run(
        &self,
        workflow_run_id: Uuid,
        pipeline_run_id: Uuid,
    ) -> Result<(), SendableError> {
        self.pool()
            .execute(
                sqlx::query(
                    &self.render("UPDATE workflow_runs SET pipeline_run_id = ? WHERE id = ?"),
                )
                .bind(pipeline_run_id)
                .bind(workflow_run_id),
            )
            .await?;
        Ok(())
    }

    async fn fetch_workflow_run(
        &self,
        workflow_run_id: Uuid,
    ) -> Result<Option<WorkflowRun>, SendableError> {
        for _ in 0..3 {
            let row = sqlx::query(&self.render(&format!(
                "SELECT {WORKFLOW_RUN_COLUMNS} FROM workflow_runs WHERE id = ?"
            )))
            .bind(workflow_run_id)
            .fetch_optional(self.pool())
            .await?;
            let Some(row) = row else { return Ok(None) };
            let mut run = mappers::row_to_workflow_run(&row);
            if let Some(state) = execution_state_sql::load(self, workflow_run_id).await? {
                run.execution_state = state;
            }
            let current =
                sqlx::query(&self.render("SELECT state_version FROM workflow_runs WHERE id = ?"))
                    .bind(workflow_run_id)
                    .fetch_one(self.pool())
                    .await?
                    .get::<i64, _>("state_version");
            if current == run.state_version {
                return Ok(Some(run));
            }
        }
        Err(Box::new(std::io::Error::other(
            "workflow execution state changed repeatedly while reading",
        )))
    }

    async fn fetch_workflow_runs_for_workflow(
        &self,
        workflow_id: Uuid,
    ) -> Result<Vec<WorkflowRun>, SendableError> {
        let rows = sqlx::query(&self.render(&format!("SELECT {WORKFLOW_RUN_COLUMNS} FROM workflow_runs WHERE workflow_id = ? ORDER BY created_at DESC, id DESC")))
            .bind(workflow_id)
            .fetch_all(self.pool())
            .await?;
        let mut runs: Vec<_> = rows.iter().map(mappers::row_to_workflow_run).collect();
        for run in &mut runs {
            if let Some(state) = execution_state_sql::load(self, run.id).await? {
                run.execution_state = state;
            }
        }
        Ok(runs)
    }

    async fn set_workflow_run_name(
        &self,
        workflow_run_id: Uuid,
        name: Option<String>,
    ) -> Result<(), SendableError> {
        self.pool()
            .execute(
                sqlx::query(&self.render("UPDATE workflow_runs SET name = ? WHERE id = ?"))
                    .bind(name)
                    .bind(workflow_run_id),
            )
            .await?;
        Ok(())
    }

    async fn set_run_correlation_key(
        &self,
        workflow_run_id: Uuid,
        correlation_key: String,
    ) -> Result<(), SendableError> {
        // write-once: only stamp a run that has no correlation key yet, so repeated stamping across
        // inline steps is idempotent.
        self.pool()
            .execute(
                sqlx::query(&self.render(
                    "UPDATE workflow_runs SET correlation_key = ? WHERE id = ? AND correlation_key IS NULL",
                ))
                .bind(correlation_key)
                .bind(workflow_run_id),
            )
            .await?;
        Ok(())
    }

    async fn update_workflow_run_status(
        &self,
        workflow_run_id: Uuid,
        status: WorkflowStatus,
        active_node_id: Option<String>,
        state: Option<WorkflowExecutionState>,
        message: Option<String>,
    ) -> Result<(), SendableError> {
        let now = Utc::now().timestamp();
        let terminal = status.is_terminal();
        let has_state = state.is_some();
        let mut tx = self.pool().begin().await?;
        let updated = sqlx::query(&self.render(
                    "UPDATE workflow_runs SET status = ?, active_node_id = COALESCE(?, active_node_id), state = CASE WHEN ? THEN '{}' ELSE state END, state_version = CASE WHEN ? THEN state_version + 1 ELSE state_version END, message = COALESCE(?, message), started_at = CASE WHEN ? = 'running' AND started_at IS NULL THEN ? ELSE started_at END, finished_at = CASE WHEN ? THEN ? ELSE finished_at END WHERE id = ? AND status NOT IN ('succeeded', 'failed', 'timed_out', 'canceled')",
                ))
                .bind(status.as_str())
                .bind(active_node_id)
                .bind(has_state)
                .bind(has_state)
                .bind(message)
                .bind(status.as_str())
                .bind(now)
                .bind(terminal)
                .bind(now)
                .bind(workflow_run_id)
                .execute(&mut *tx)
                .await?;
        if updated.affected() > 0
            && let Some(state) = state
        {
            execution_state_sql::write(self, &mut *tx, workflow_run_id, &state, true).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    async fn update_workflow_run_status_cas(
        &self,
        workflow_run_id: Uuid,
        expected_version: i64,
        status: WorkflowStatus,
        active_node_id: Option<String>,
        state: WorkflowExecutionState,
        message: Option<String>,
    ) -> Result<bool, SendableError> {
        let now = Utc::now().timestamp();
        let terminal = status.is_terminal();
        let mut tx = self.pool().begin().await?;
        let result = sqlx::query(&self.render(
                    "UPDATE workflow_runs SET status = ?, active_node_id = COALESCE(?, active_node_id), state = '{}', state_version = state_version + 1, message = COALESCE(?, message), started_at = CASE WHEN ? = 'running' AND started_at IS NULL THEN ? ELSE started_at END, finished_at = CASE WHEN ? THEN ? ELSE finished_at END WHERE id = ? AND state_version = ?",
                ))
                .bind(status.as_str())
                .bind(active_node_id)
                .bind(message)
                .bind(status.as_str())
                .bind(now)
                .bind(terminal)
                .bind(now)
                .bind(workflow_run_id)
                .bind(expected_version)
                .execute(&mut *tx)
                .await?;
        if result.affected() == 0 {
            tx.rollback().await?;
            return Ok(false);
        }
        execution_state_sql::write(self, &mut *tx, workflow_run_id, &state, true).await?;
        tx.commit().await?;
        Ok(true)
    }

    async fn update_workflow_run_execution_state_cas(
        &self,
        workflow_run_id: Uuid,
        expected_version: i64,
        state: WorkflowExecutionState,
    ) -> Result<bool, SendableError> {
        let mut tx = self.pool().begin().await?;
        let result = sqlx::query(&self.render(
                    "UPDATE workflow_runs SET state = '{}', state_version = state_version + 1 WHERE id = ? AND state_version = ?",
                ))
                .bind(workflow_run_id)
                .bind(expected_version)
                .execute(&mut *tx)
                .await?;
        // no row matched: another writer bumped the version between the caller's read and this
        // write, so the caller's blob is stale and must be rebuilt.
        if result.affected() == 0 {
            tx.rollback().await?;
            return Ok(false);
        }
        execution_state_sql::write(self, &mut *tx, workflow_run_id, &state, true).await?;
        tx.commit().await?;
        Ok(true)
    }

    async fn migrate_workflow_execution_states(&self) -> Result<(), SendableError> {
        let rows = sqlx::query(&self.render(
            "SELECT r.id FROM workflow_runs r LEFT JOIN workflow_run_execution_states s ON s.workflow_run_id = r.id WHERE s.workflow_run_id IS NULL OR r.state <> '{}' ORDER BY r.created_at, r.id",
        ))
        .fetch_all(self.pool())
        .await?;
        for row in rows {
            let workflow_run_id = row.get::<Uuid, _>("id");
            let mut tx = self.pool().begin().await?;
            // every engine replica performs this startup backfill. serialize on the run row so two
            // replicas cannot both observe the normalized projection missing and race its insert.
            // sqlite has no `FOR UPDATE`, so a no-op write acquires its database writer lock first.
            if self.dialect() == SqlDialect::Sqlite {
                sqlx::query(&self.render("UPDATE workflow_runs SET state = state WHERE id = ?"))
                    .bind(workflow_run_id)
                    .execute(&mut *tx)
                    .await?;
            }
            let lock = if self.dialect() == SqlDialect::Sqlite {
                ""
            } else {
                " FOR UPDATE"
            };
            let Some(locked) = sqlx::query(&self.render(&format!(
                "SELECT state FROM workflow_runs WHERE id = ?{lock}"
            )))
            .bind(workflow_run_id)
            .fetch_optional(&mut *tx)
            .await?
            else {
                tx.rollback().await?;
                continue;
            };
            let normalized = sqlx::query(&self.render(
                "SELECT workflow_run_id FROM workflow_run_execution_states WHERE workflow_run_id = ?",
            ))
            .bind(workflow_run_id)
            .fetch_optional(&mut *tx)
            .await?;
            if normalized.is_some() {
                sqlx::query(&self.render("UPDATE workflow_runs SET state = '{}' WHERE id = ?"))
                    .bind(workflow_run_id)
                    .execute(&mut *tx)
                    .await?;
                tx.commit().await?;
                continue;
            }
            let legacy = Value::from(
                serde_json::from_str::<serde_json::Value>(&locked.get::<String, _>("state"))
                    .unwrap_or_default(),
            );
            let state = WorkflowExecutionState::from_state(&legacy);
            execution_state_sql::write(self, &mut *tx, workflow_run_id, &state, false).await?;
            sqlx::query(&self.render("UPDATE workflow_runs SET state = '{}' WHERE id = ?"))
                .bind(workflow_run_id)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
        }
        Ok(())
    }

    async fn claim_cooldown(
        &self,
        name: String,
        window_seconds: i64,
        now_unix: i64,
    ) -> Result<Option<i64>, SendableError> {
        // a window is claimable once `last_run_at` is at or before this.
        let cutoff = now_unix.saturating_sub(window_seconds.max(0));

        // take an existing, elapsed window. the predicate and the stamp are the same statement, so
        // two racers cannot both satisfy it: the second blocks on the row lock, re-evaluates against
        // the winner's `last_run_at = now`, and matches nothing.
        let taken = sqlx::query(&self.render(
            "UPDATE workflow_cooldowns SET last_run_at = ? WHERE name = ? AND last_run_at <= ?",
        ))
        .bind(now_unix)
        .bind(name.as_str())
        .bind(cutoff)
        .execute(self.pool())
        .await?;
        if taken.affected() > 0 {
            return Ok(None);
        }

        // no row yet: first use of this gate. insert-or-ignore settles the race between two callers
        // both finding it absent — exactly one insert lands.
        let created = sqlx::query(&self.render(&self.dialect().insert_ignore(
            "workflow_cooldowns",
            "name, last_run_at",
            "?, ?",
            "name",
            None,
        )))
        .bind(name.as_str())
        .bind(now_unix)
        .execute(self.pool())
        .await?;
        if created.affected() > 0 {
            return Ok(None);
        }

        // lost: somebody holds the window. read it back for the caller's benefit only — this value
        // is reported, never acted on, so a stale read here cannot admit anyone.
        let row =
            sqlx::query(&self.render("SELECT last_run_at FROM workflow_cooldowns WHERE name = ?"))
                .bind(name.as_str())
                .fetch_optional(self.pool())
                .await?;
        let last_run_at = row
            .map(|row| row.get::<i64, _>("last_run_at"))
            .unwrap_or(now_unix);
        Ok(Some((last_run_at + window_seconds - now_unix).max(0)))
    }

    async fn create_automation_record(
        &self,
        record_type: String,
        record: Value,
    ) -> Result<Value, SendableError> {
        let now = Utc::now().timestamp();
        let id = Uuid::now_v7();
        let columns = "id, record_type, data, created_at, updated_at";
        if self.dialect() == SqlDialect::MySql {
            let mut conn = self.pool().acquire().await?;
            sqlx::query(&self.render(
                "INSERT INTO automation_records (id, record_type, workflow_run_id, external_item_id, node_id, provider, resource_type, external_id, status, title, url, body, path, prompt, approval_type, resolved_by, resolved_at, metadata, data, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            ))
            .bind(id)
            .bind(record_type)
            .bind(json_opt_uuid(&record, "workflow_run_id"))
            .bind(json_opt_uuid(&record, "external_item_id"))
            .bind(json_opt_str(&record, "node_id"))
            .bind(json_str(&record, "provider"))
            .bind(json_str(&record, "resource_type"))
            .bind(json_str(&record, "external_id"))
            .bind(json_str(&record, "status"))
            .bind(json_opt_str(&record, "title"))
            .bind(json_opt_str(&record, "url"))
            .bind(json_opt_str(&record, "body"))
            .bind(json_opt_str(&record, "path"))
            .bind(json_opt_str(&record, "prompt"))
            .bind(json_opt_str(&record, "approval_type"))
            .bind(json_opt_str(&record, "resolved_by"))
            .bind(json_opt_i64(&record, "resolved_at"))
            .bind(json_metadata(&record))
            .bind(record.to_string())
            .bind(now)
            .bind(now)
            .execute(&mut *conn)
            .await?;
            let row = sqlx::query(&self.render(&format!(
                "SELECT {columns} FROM automation_records WHERE id = ?"
            )))
            .bind(id)
            .fetch_one(&mut *conn)
            .await?;
            return Ok(mappers::row_to_automation_record(&row));
        }
        let row = sqlx::query(&self.render(
            "INSERT INTO automation_records (id, record_type, workflow_run_id, external_item_id, node_id, provider, resource_type, external_id, status, title, url, body, path, prompt, approval_type, resolved_by, resolved_at, metadata, data, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             RETURNING id, record_type, data, created_at, updated_at",
        ))
        .bind(id)
        .bind(record_type)
        .bind(json_opt_uuid(&record, "workflow_run_id"))
        .bind(json_opt_uuid(&record, "external_item_id"))
        .bind(json_opt_str(&record, "node_id"))
        .bind(json_str(&record, "provider"))
        .bind(json_str(&record, "resource_type"))
        .bind(json_str(&record, "external_id"))
        .bind(json_str(&record, "status"))
        .bind(json_opt_str(&record, "title"))
        .bind(json_opt_str(&record, "url"))
        .bind(json_opt_str(&record, "body"))
        .bind(json_opt_str(&record, "path"))
        .bind(json_opt_str(&record, "prompt"))
        .bind(json_opt_str(&record, "approval_type"))
        .bind(json_opt_str(&record, "resolved_by"))
        .bind(json_opt_i64(&record, "resolved_at"))
        .bind(json_metadata(&record))
        .bind(record.to_string())
        .bind(now)
        .bind(now)
        .fetch_one(self.pool())
        .await?;
        Ok(mappers::row_to_automation_record(&row))
    }

    async fn update_automation_record(
        &self,
        record_type: String,
        record_id: Uuid,
        record: Value,
    ) -> Result<Value, SendableError> {
        let now = Utc::now().timestamp();
        let columns = "id, record_type, data, created_at, updated_at";
        if self.dialect() == SqlDialect::MySql {
            sqlx::query(&self.render(
                "UPDATE automation_records SET workflow_run_id = ?, external_item_id = ?, node_id = ?, provider = ?, resource_type = ?, external_id = ?, status = ?, title = ?, url = ?, body = ?, path = ?, prompt = ?, approval_type = ?, resolved_by = ?, resolved_at = ?, metadata = ?, data = ?, updated_at = ? WHERE id = ? AND record_type = ?",
            ))
            .bind(json_opt_uuid(&record, "workflow_run_id"))
            .bind(json_opt_uuid(&record, "external_item_id"))
            .bind(json_opt_str(&record, "node_id"))
            .bind(json_str(&record, "provider"))
            .bind(json_str(&record, "resource_type"))
            .bind(json_str(&record, "external_id"))
            .bind(json_str(&record, "status"))
            .bind(json_opt_str(&record, "title"))
            .bind(json_opt_str(&record, "url"))
            .bind(json_opt_str(&record, "body"))
            .bind(json_opt_str(&record, "path"))
            .bind(json_opt_str(&record, "prompt"))
            .bind(json_opt_str(&record, "approval_type"))
            .bind(json_opt_str(&record, "resolved_by"))
            .bind(json_opt_i64(&record, "resolved_at"))
            .bind(json_metadata(&record))
            .bind(record.to_string())
            .bind(now)
            .bind(record_id)
            .bind(record_type.as_str())
            .execute(self.pool())
            .await?;
            let row = sqlx::query(&self.render(&format!(
                "SELECT {columns} FROM automation_records WHERE id = ? AND record_type = ?",
            )))
            .bind(record_id)
            .bind(record_type)
            .fetch_one(self.pool())
            .await?;
            return Ok(mappers::row_to_automation_record(&row));
        }
        let row = sqlx::query(&self.render(
            "UPDATE automation_records SET workflow_run_id = ?, external_item_id = ?, node_id = ?, provider = ?, resource_type = ?, external_id = ?, status = ?, title = ?, url = ?, body = ?, path = ?, prompt = ?, approval_type = ?, resolved_by = ?, resolved_at = ?, metadata = ?, data = ?, updated_at = ? WHERE id = ? AND record_type = ? RETURNING id, record_type, data, created_at, updated_at",
        ))
        .bind(json_opt_uuid(&record, "workflow_run_id"))
        .bind(json_opt_uuid(&record, "external_item_id"))
        .bind(json_opt_str(&record, "node_id"))
        .bind(json_str(&record, "provider"))
        .bind(json_str(&record, "resource_type"))
        .bind(json_str(&record, "external_id"))
        .bind(json_str(&record, "status"))
        .bind(json_opt_str(&record, "title"))
        .bind(json_opt_str(&record, "url"))
        .bind(json_opt_str(&record, "body"))
        .bind(json_opt_str(&record, "path"))
        .bind(json_opt_str(&record, "prompt"))
        .bind(json_opt_str(&record, "approval_type"))
        .bind(json_opt_str(&record, "resolved_by"))
        .bind(json_opt_i64(&record, "resolved_at"))
        .bind(json_metadata(&record))
        .bind(record.to_string())
        .bind(now)
        .bind(record_id)
        .bind(record_type)
        .fetch_one(self.pool())
        .await?;
        Ok(mappers::row_to_automation_record(&row))
    }

    async fn create_gate(&self, record: Value) -> Result<Value, SendableError> {
        let now = Utc::now().timestamp();
        let id = Uuid::now_v7();
        let columns = "id, data, created_at, updated_at";
        if self.dialect() == SqlDialect::MySql {
            let mut conn = self.pool().acquire().await?;
            sqlx::query(&self.render(
                "INSERT INTO gates (id, workflow_run_id, node_id, kind, status, label, reason, resolved_by, resolved_at, metadata, data, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            ))
            .bind(id)
            .bind(json_opt_uuid(&record, "workflow_run_id"))
            .bind(json_str(&record, "node_id"))
            .bind(json_str(&record, "kind"))
            .bind(json_str(&record, "status"))
            .bind(json_opt_str(&record, "label"))
            .bind(json_opt_str(&record, "reason"))
            .bind(json_opt_str(&record, "resolved_by"))
            .bind(json_opt_i64(&record, "resolved_at"))
            .bind(json_metadata(&record))
            .bind(record.to_string())
            .bind(now)
            .bind(now)
            .execute(&mut *conn)
            .await?;
            let row =
                sqlx::query(&self.render(&format!("SELECT {columns} FROM gates WHERE id = ?")))
                    .bind(id)
                    .fetch_one(&mut *conn)
                    .await?;
            return Ok(mappers::row_to_gate(&row));
        }
        let row = sqlx::query(&self.render(
            "INSERT INTO gates (id, workflow_run_id, node_id, kind, status, label, reason, resolved_by, resolved_at, metadata, data, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             RETURNING id, data, created_at, updated_at",
        ))
        .bind(id)
        .bind(json_opt_uuid(&record, "workflow_run_id"))
        .bind(json_str(&record, "node_id"))
        .bind(json_str(&record, "kind"))
        .bind(json_str(&record, "status"))
        .bind(json_opt_str(&record, "label"))
        .bind(json_opt_str(&record, "reason"))
        .bind(json_opt_str(&record, "resolved_by"))
        .bind(json_opt_i64(&record, "resolved_at"))
        .bind(json_metadata(&record))
        .bind(record.to_string())
        .bind(now)
        .bind(now)
        .fetch_one(self.pool())
        .await?;
        Ok(mappers::row_to_gate(&row))
    }

    async fn update_gate(&self, gate_id: Uuid, record: Value) -> Result<Value, SendableError> {
        let now = Utc::now().timestamp();
        let columns = "id, data, created_at, updated_at";
        if self.dialect() == SqlDialect::MySql {
            sqlx::query(&self.render(
                "UPDATE gates SET node_id = ?, kind = ?, status = ?, label = ?, reason = ?, resolved_by = ?, resolved_at = ?, metadata = ?, data = ?, updated_at = ? WHERE id = ?",
            ))
            .bind(json_str(&record, "node_id"))
            .bind(json_str(&record, "kind"))
            .bind(json_str(&record, "status"))
            .bind(json_opt_str(&record, "label"))
            .bind(json_opt_str(&record, "reason"))
            .bind(json_opt_str(&record, "resolved_by"))
            .bind(json_opt_i64(&record, "resolved_at"))
            .bind(json_metadata(&record))
            .bind(record.to_string())
            .bind(now)
            .bind(gate_id)
            .execute(self.pool())
            .await?;
            let row =
                sqlx::query(&self.render(&format!("SELECT {columns} FROM gates WHERE id = ?")))
                    .bind(gate_id)
                    .fetch_one(self.pool())
                    .await?;
            return Ok(mappers::row_to_gate(&row));
        }
        let row = sqlx::query(&self.render(
            "UPDATE gates SET node_id = ?, kind = ?, status = ?, label = ?, reason = ?, resolved_by = ?, resolved_at = ?, metadata = ?, data = ?, updated_at = ? WHERE id = ? RETURNING id, data, created_at, updated_at",
        ))
        .bind(json_str(&record, "node_id"))
        .bind(json_str(&record, "kind"))
        .bind(json_str(&record, "status"))
        .bind(json_opt_str(&record, "label"))
        .bind(json_opt_str(&record, "reason"))
        .bind(json_opt_str(&record, "resolved_by"))
        .bind(json_opt_i64(&record, "resolved_at"))
        .bind(json_metadata(&record))
        .bind(record.to_string())
        .bind(now)
        .bind(gate_id)
        .fetch_one(self.pool())
        .await?;
        Ok(mappers::row_to_gate(&row))
    }

    async fn fetch_gate(&self, gate_id: Uuid) -> Result<Option<Value>, SendableError> {
        let row = sqlx::query(
            &self.render("SELECT id, data, created_at, updated_at FROM gates WHERE id = ?"),
        )
        .bind(gate_id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|row| mappers::row_to_gate(&row)))
    }

    async fn record_audit_log(&self, record: Value) -> Result<Value, SendableError> {
        let now = Utc::now().timestamp();
        let id = Uuid::now_v7();
        sqlx::query(&self.render(
            "INSERT INTO audit_log (id, actor_id, actor_kind, action, resource_type, resource_id, outcome, detail, metadata, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        ))
        .bind(id)
        .bind(json_opt_uuid(&record, "actor_id"))
        .bind(json_str(&record, "actor_kind"))
        .bind(json_str(&record, "action"))
        .bind(json_opt_str(&record, "resource_type"))
        .bind(json_opt_uuid(&record, "resource_id"))
        .bind(json_str(&record, "outcome"))
        .bind(json_opt_str(&record, "detail"))
        .bind(json_metadata(&record))
        .bind(now)
        .execute(self.pool())
        .await?;
        let row = sqlx::query(&self.render(
            "SELECT id, actor_id, actor_kind, action, resource_type, resource_id, outcome, detail, metadata, created_at FROM audit_log WHERE id = ?",
        ))
        .bind(id)
        .fetch_one(self.pool())
        .await?;
        Ok(mappers::row_to_audit_log(&row))
    }

    async fn list_settings(&self) -> Result<Vec<SettingRecord>, SendableError> {
        let rows = sqlx::query(
            "SELECT kind, scope, name, value, updated_at FROM settings ORDER BY kind, scope, name",
        )
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_setting).collect())
    }

    async fn fetch_org(&self, id: Uuid) -> Result<Option<Organization>, SendableError> {
        let row = sqlx::query(&self.render(
            "SELECT id, name, slug, disabled, created_at, updated_at FROM organizations WHERE id = ?",
        ))
        .bind(id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.as_ref().map(mappers::row_to_organization))
    }

    async fn list_org_resource_groups(
        &self,
        org_id: Uuid,
    ) -> Result<Vec<OrgResourceGroup>, SendableError> {
        let rows = sqlx::query(&self.render(
            "SELECT org_id, backend, kind, desired, dedicated FROM org_resource_groups \
             WHERE org_id = ? ORDER BY backend, kind",
        ))
        .bind(org_id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows
            .iter()
            .map(mappers::row_to_org_resource_group)
            .collect())
    }

    async fn fetch_workflow_by_name(
        &self,
        name: String,
    ) -> Result<Option<WorkflowDefinition>, SendableError> {
        // match either an unqualified `name` or a qualified subflow target `"<namespace>.<name>"`
        // against the stored identity `namespace + "." + name`. matching the concatenation (rather
        // than splitting the target) is unambiguous when a workflow name itself contains dots.
        let concat = if self.dialect() == SqlDialect::MySql {
            "CONCAT(namespace, '.', name)"
        } else {
            "namespace || '.' || name"
        };
        let sql = format!(
            "SELECT id, name, namespace, org_id, version, enabled, input_schema, definition, created_at, updated_at \
             FROM workflows WHERE name = ? OR (namespace IS NOT NULL AND {concat} = ?) \
             ORDER BY created_at, id LIMIT 1"
        );
        let row = sqlx::query(&self.render(&sql))
            .bind(&name)
            .bind(&name)
            .fetch_optional(self.pool())
            .await?;
        Ok(row.map(|row| mappers::row_to_workflow(&row)))
    }

    async fn try_record_pipeline_trigger_firing(
        &self,
        trigger_id: Uuid,
        fire_key: String,
    ) -> Result<bool, SendableError> {
        let firing_sql = self.render(&self.dialect().insert_ignore(
            "pipeline_trigger_firings",
            "id, trigger_id, fire_key, scheduler_id, created_at",
            "?, ?, ?, ?, ?",
            "trigger_id, fire_key",
            None,
        ));
        let insert = sqlx::query(&firing_sql)
            .bind(Uuid::now_v7())
            .bind(trigger_id)
            .bind(fire_key.as_str())
            .bind("chained")
            .bind(Utc::now().timestamp())
            .execute(self.pool())
            .await?;
        Ok(insert.affected() > 0)
    }

    async fn create_pipeline_run(
        &self,
        pipeline_id: Uuid,
        pipeline_snapshot: Pipeline,
        parameters: Value,
        state: Value,
        provenance: WorkflowRunProvenance,
    ) -> Result<PipelineRun, SendableError> {
        let id = Uuid::now_v7();
        let created_at = Utc::now().timestamp();
        let snapshot_json = serde_json::to_string(&pipeline_snapshot)?;
        let source_kind = provenance.source_kind.map(|v| v.as_str().to_string());
        let actor_type = provenance.actor_type.map(|v| v.as_str().to_string());
        if self.dialect() == SqlDialect::MySql {
            let mut conn = self.pool().acquire().await?;
            sqlx::query(&self.render(
                "INSERT INTO pipeline_runs (id, pipeline_id, pipeline_snapshot, status, parameters, state, created_at, trigger_source_kind, trigger_actor_type, trigger_actor_replica_id, trigger_actor_display_name, trigger_metadata) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            ))
            .bind(id)
            .bind(pipeline_id)
            .bind(&snapshot_json)
            .bind(WorkflowStatus::Queued.as_str())
            .bind(parameters.to_string())
            .bind(state.to_string())
            .bind(created_at)
            .bind(source_kind)
            .bind(actor_type)
            .bind(provenance.actor_replica_id)
            .bind(provenance.actor_display_name)
            .bind(provenance.metadata.to_string())
            .execute(&mut *conn)
            .await?;
            let row = sqlx::query(&self.render(&format!(
                "SELECT {PIPELINE_RUN_COLUMNS} FROM pipeline_runs WHERE id = ?"
            )))
            .bind(id)
            .fetch_one(&mut *conn)
            .await?;
            return Ok(mappers::row_to_pipeline_run(&row));
        }
        let row = sqlx::query(&self.render(&format!(
            "INSERT INTO pipeline_runs (id, pipeline_id, pipeline_snapshot, status, parameters, state, created_at, trigger_source_kind, trigger_actor_type, trigger_actor_replica_id, trigger_actor_display_name, trigger_metadata)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             RETURNING {PIPELINE_RUN_COLUMNS}",
        )))
        .bind(id)
        .bind(pipeline_id)
        .bind(serde_json::to_string(&pipeline_snapshot)?)
        .bind(WorkflowStatus::Queued.as_str())
        .bind(parameters.to_string())
        .bind(state.to_string())
        .bind(created_at)
        .bind(source_kind)
        .bind(actor_type)
        .bind(provenance.actor_replica_id)
        .bind(provenance.actor_display_name)
        .bind(provenance.metadata.to_string())
        .fetch_one(self.pool())
        .await?;
        Ok(mappers::row_to_pipeline_run(&row))
    }

    async fn discard_queued_pipeline_run(
        &self,
        pipeline_run_id: Uuid,
    ) -> Result<(), SendableError> {
        sqlx::query(&self.render("DELETE FROM pipeline_runs WHERE id = ? AND status = 'queued'"))
            .bind(pipeline_run_id)
            .execute(self.pool())
            .await?;
        Ok(())
    }

    async fn create_pipeline_member_attempt(
        &self,
        pipeline_run_id: Uuid,
        member_key: String,
        workflow_id: Uuid,
        attempt: i64,
        parameters: Value,
    ) -> Result<Option<PipelineMemberAttempt>, SendableError> {
        let id = Uuid::now_v7();
        let now = Utc::now().timestamp();
        let sql = self.render(&self.dialect().insert_ignore(
            "pipeline_member_attempts",
            "id, pipeline_run_id, member_key, workflow_id, attempt, workflow_run_id, status, parameters, result, message, created_at, started_at, finished_at",
            "?, ?, ?, ?, ?, NULL, 'pending', ?, 'null', NULL, ?, NULL, NULL",
            "pipeline_run_id, member_key, attempt",
            None,
        ));
        let inserted = sqlx::query(&sql)
            .bind(id)
            .bind(pipeline_run_id)
            .bind(member_key)
            .bind(workflow_id)
            .bind(attempt)
            .bind(parameters.to_string())
            .bind(now)
            .execute(self.pool())
            .await?;
        if inserted.affected() == 0 {
            return Ok(None);
        }
        let row = sqlx::query(&self.render(&format!(
            "SELECT {PIPELINE_MEMBER_ATTEMPT_COLUMNS} FROM pipeline_member_attempts WHERE id = ?"
        )))
        .bind(id)
        .fetch_one(self.pool())
        .await?;
        Ok(Some(mappers::row_to_pipeline_member_attempt(&row)))
    }

    async fn bind_pipeline_member_attempt_run(
        &self,
        attempt_id: Uuid,
        workflow_run_id: Uuid,
    ) -> Result<(), SendableError> {
        let now = Utc::now().timestamp();
        sqlx::query(&self.render("UPDATE pipeline_member_attempts SET workflow_run_id = ?, status = 'running', started_at = COALESCE(started_at, ?) WHERE id = ?"))
            .bind(workflow_run_id).bind(now).bind(attempt_id).execute(self.pool()).await?;
        Ok(())
    }

    async fn update_pipeline_member_attempt(
        &self,
        attempt_id: Uuid,
        status: PipelineMemberAttemptStatus,
        result: Value,
        message: Option<String>,
    ) -> Result<(), SendableError> {
        let finished = status.is_terminal().then(|| Utc::now().timestamp());
        sqlx::query(&self.render("UPDATE pipeline_member_attempts SET status = ?, result = ?, message = ?, finished_at = COALESCE(?, finished_at) WHERE id = ?"))
            .bind(status.as_str()).bind(result.to_string()).bind(message).bind(finished).bind(attempt_id)
            .execute(self.pool()).await?;
        Ok(())
    }

    async fn fetch_pipeline_member_attempts(
        &self,
        pipeline_run_id: Uuid,
    ) -> Result<Vec<PipelineMemberAttempt>, SendableError> {
        let rows = sqlx::query(&self.render(&format!(
            "SELECT {PIPELINE_MEMBER_ATTEMPT_COLUMNS} FROM pipeline_member_attempts WHERE pipeline_run_id = ? ORDER BY member_key, attempt"
        )))
        .bind(pipeline_run_id).fetch_all(self.pool()).await?;
        Ok(rows
            .iter()
            .map(mappers::row_to_pipeline_member_attempt)
            .collect())
    }

    async fn delete_unstarted_pipeline_member_attempts(
        &self,
        pipeline_run_id: Uuid,
        member_key: String,
    ) -> Result<(), SendableError> {
        sqlx::query(&self.render(
            "DELETE FROM pipeline_member_attempts WHERE pipeline_run_id = ? AND member_key = ? AND workflow_run_id IS NULL",
        ))
        .bind(pipeline_run_id)
        .bind(member_key)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    async fn fetch_workflow_runs_for_pipeline_run(
        &self,
        pipeline_run_id: Uuid,
    ) -> Result<Vec<WorkflowRun>, SendableError> {
        let rows = sqlx::query(&self.render(&format!(
            "SELECT {WORKFLOW_RUN_COLUMNS} FROM workflow_runs WHERE pipeline_run_id = ? ORDER BY created_at, id"
        )))
        .bind(pipeline_run_id)
        .fetch_all(self.pool())
        .await?;
        let mut runs: Vec<_> = rows.iter().map(mappers::row_to_workflow_run).collect();
        for run in &mut runs {
            if let Some(state) = execution_state_sql::load(self, run.id).await? {
                run.execution_state = state;
            }
        }
        Ok(runs)
    }

    async fn try_record_trigger_firing(
        &self,
        trigger_id: Uuid,
        fire_key: String,
    ) -> Result<bool, SendableError> {
        // insert-ignore on the unique (trigger_id, fire_key); a zero-row insert means another
        // caller already recorded this firing, so the caller must not start a duplicate run.
        let firing_sql = self.render(&self.dialect().insert_ignore(
            "workflow_trigger_firings",
            "id, trigger_id, fire_key, scheduler_id, created_at",
            "?, ?, ?, ?, ?",
            "trigger_id, fire_key",
            None,
        ));
        let insert = sqlx::query(&firing_sql)
            .bind(Uuid::now_v7())
            .bind(trigger_id)
            .bind(fire_key.as_str())
            .bind("chained")
            .bind(Utc::now().timestamp())
            .execute(self.pool())
            .await?;
        Ok(insert.affected() > 0)
    }

    async fn create_workflow_run(
        &self,
        workflow_id: Uuid,
        workflow_snapshot: WorkflowDefinition,
        parameters: Value,
        state: Value,
        name: Option<String>,
        provenance: WorkflowRunProvenance,
    ) -> Result<WorkflowRun, SendableError> {
        let snapshot = serde_json::to_string(&workflow_snapshot)?;
        let state = WorkflowExecutionState::from_state(&state);
        let id = Uuid::now_v7();
        let created_at = Utc::now().timestamp();
        let mut tx = self.pool().begin().await?;
        sqlx::query(&self.render(
            "INSERT INTO workflow_runs (id, workflow_id, workflow_snapshot, status, active_node_id, parameters, state, created_at, name, trigger_source_kind, trigger_actor_type, trigger_actor_replica_id, trigger_actor_display_name, trigger_request_host, trigger_request_ip, trigger_metadata) VALUES (?, ?, ?, ?, NULL, ?, '{}', ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        ))
        .bind(id)
        .bind(workflow_id)
        .bind(snapshot)
        .bind(WorkflowStatus::Queued.as_str())
        .bind(parameters.to_string())
        .bind(created_at)
        .bind(name)
        .bind(provenance.source_kind.map(|value| value.as_str().to_string()))
        .bind(provenance.actor_type.map(|value| value.as_str().to_string()))
        .bind(provenance.actor_replica_id)
        .bind(provenance.actor_display_name)
        .bind(provenance.request_host)
        .bind(provenance.request_ip)
        .bind(provenance.metadata.to_string())
        .execute(&mut *tx)
        .await?;
        execution_state_sql::write(self, &mut *tx, id, &state, false).await?;
        let row = sqlx::query(&self.render(&format!(
            "SELECT {WORKFLOW_RUN_COLUMNS} FROM workflow_runs WHERE id = ?"
        )))
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        let mut run = mappers::row_to_workflow_run(&row);
        run.execution_state = state;
        tx.commit().await?;
        Ok(run)
    }

    async fn fetch_workflow_runs_by_name(
        &self,
        name: String,
        open_only: bool,
    ) -> Result<Vec<WorkflowRun>, SendableError> {
        let rows = if open_only {
            sqlx::query(&self.render(&format!("SELECT {WORKFLOW_RUN_COLUMNS} FROM workflow_runs WHERE name = ? AND status NOT IN ('succeeded', 'failed', 'timed_out', 'canceled') ORDER BY created_at DESC, id DESC")))
                .bind(name)
                .fetch_all(self.pool())
                .await?
        } else {
            sqlx::query(&self.render(&format!(
                "SELECT {WORKFLOW_RUN_COLUMNS} FROM workflow_runs WHERE name = ? ORDER BY created_at DESC, id DESC"
            )))
            .bind(name)
            .fetch_all(self.pool())
            .await?
        };
        let mut runs: Vec<_> = rows.iter().map(mappers::row_to_workflow_run).collect();
        for run in &mut runs {
            if let Some(state) = execution_state_sql::load(self, run.id).await? {
                run.execution_state = state;
            }
        }
        Ok(runs)
    }

    async fn fetch_automation_records(
        &self,
        record_type: String,
        workflow_run_id: Option<Uuid>,
        external_item_id: Option<Uuid>,
    ) -> Result<Vec<Value>, SendableError> {
        let rows = sqlx::query(&self.render("SELECT id, record_type, data, created_at, updated_at FROM automation_records WHERE record_type = ? ORDER BY created_at DESC, id DESC"))
            .bind(record_type)
            .fetch_all(self.pool())
            .await?;
        Ok(rows
            .iter()
            .map(mappers::row_to_automation_record)
            .filter(|record| {
                workflow_run_id.is_none_or(|id| {
                    record.get("workflow_run_id").and_then(Value::as_str)
                        == Some(id.to_string().as_str())
                }) && external_item_id.is_none_or(|id| {
                    record.get("external_item_id").and_then(Value::as_str)
                        == Some(id.to_string().as_str())
                })
            })
            .collect())
    }

    async fn fetch_setting(
        &self,
        kind: SettingKind,
        scope: String,
        name: String,
    ) -> Result<Option<SettingRecord>, SendableError> {
        let row = sqlx::query(&self.render(
            "SELECT kind, scope, name, value, updated_at FROM settings WHERE kind = ? AND scope = ? AND name = ?",
        ))
        .bind(kind.as_str())
        .bind(scope)
        .bind(name)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|row| mappers::row_to_setting(&row)))
    }
}
