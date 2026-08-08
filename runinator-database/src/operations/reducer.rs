//! the operations the workflow state machine calls.
//!
//! the `ReducerStore` half of the generic sql implementation. bodies are written once, over any
//! `SqlBackend`; see `super` for the shared helpers they call.

use super::*;

// the bound list is repeated verbatim in every role impl in this directory. it stays spelled out
// rather than hidden behind a macro so that type errors inside the query bodies — the part that
// actually gets edited — keep pointing at real source lines instead of a macro expansion.
impl<B> ReducerStore for SqlStore<B>
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
            queries::bool_true(self.dialect()),
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
        let row = sqlx::query(&self.render(&format!(
            "SELECT {WORKFLOW_RUN_COLUMNS} FROM workflow_runs WHERE id = ?"
        )))
        .bind(workflow_run_id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|row| mappers::row_to_workflow_run(&row)))
    }

    async fn fetch_workflow_runs_for_workflow(
        &self,
        workflow_id: Uuid,
    ) -> Result<Vec<WorkflowRun>, SendableError> {
        let rows = sqlx::query(&self.render(&format!("SELECT {WORKFLOW_RUN_COLUMNS} FROM workflow_runs WHERE workflow_id = ? ORDER BY created_at DESC, id DESC")))
            .bind(workflow_id)
            .fetch_all(self.pool())
            .await?;
        Ok(rows.iter().map(mappers::row_to_workflow_run).collect())
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
        state: Option<Value>,
        message: Option<String>,
    ) -> Result<(), SendableError> {
        let now = Utc::now().timestamp();
        let terminal = status.is_terminal();
        let state_json = state.map(|value| value.to_string());
        self.pool()
            .execute(
                // `state_version` moves whenever `state` does, so a compare-and-swap writer that
                // read an older blob cannot land on top of this write.
                sqlx::query(&self.render(
                    "UPDATE workflow_runs SET status = ?, active_node_id = COALESCE(?, active_node_id), state = COALESCE(?, state), state_version = CASE WHEN ? IS NULL THEN state_version ELSE state_version + 1 END, message = COALESCE(?, message), started_at = CASE WHEN ? = 'running' AND started_at IS NULL THEN ? ELSE started_at END, finished_at = CASE WHEN ? THEN ? ELSE finished_at END WHERE id = ?",
                ))
                .bind(status.as_str())
                .bind(active_node_id)
                .bind(state_json.clone())
                .bind(state_json)
                .bind(message)
                .bind(status.as_str())
                .bind(now)
                .bind(terminal)
                .bind(now)
                .bind(workflow_run_id),
            )
            .await?;
        // a run that just reached a terminal state can still own pending ready nodes (poll re-arms,
        // timeout wakes, unclaimed siblings). left behind they are rescanned forever by the wake
        // publisher — re-driving a dead run, spamming ui events, and starving new work behind their
        // stale `ready_at`. settle them here so the backstop stops seeing them.
        if terminal {
            self.pool()
                .execute(
                    sqlx::query(&self.render(
                        "UPDATE workflow_ready_nodes SET completed_at = ?, status = 'succeeded', updated_at = ? WHERE workflow_run_id = ? AND completed_at IS NULL",
                    ))
                    .bind(now)
                    .bind(now)
                    .bind(workflow_run_id),
                )
                .await?;
        }
        Ok(())
    }

    async fn update_workflow_run_status_cas(
        &self,
        workflow_run_id: Uuid,
        expected_version: i64,
        status: WorkflowStatus,
        active_node_id: Option<String>,
        state: Value,
        message: Option<String>,
    ) -> Result<bool, SendableError> {
        let now = Utc::now().timestamp();
        let terminal = status.is_terminal();
        let result = self
            .pool()
            .execute(
                sqlx::query(&self.render(
                    "UPDATE workflow_runs SET status = ?, active_node_id = COALESCE(?, active_node_id), state = ?, state_version = state_version + 1, message = COALESCE(?, message), started_at = CASE WHEN ? = 'running' AND started_at IS NULL THEN ? ELSE started_at END, finished_at = CASE WHEN ? THEN ? ELSE finished_at END WHERE id = ? AND state_version = ?",
                ))
                .bind(status.as_str())
                .bind(active_node_id)
                .bind(state.to_string())
                .bind(message)
                .bind(status.as_str())
                .bind(now)
                .bind(terminal)
                .bind(now)
                .bind(workflow_run_id)
                .bind(expected_version),
            )
            .await?;
        Ok(result.affected() > 0)
    }

    async fn update_workflow_run_state_cas(
        &self,
        workflow_run_id: Uuid,
        expected_version: i64,
        state: Value,
    ) -> Result<bool, SendableError> {
        let result = self
            .pool()
            .execute(
                sqlx::query(&self.render(
                    "UPDATE workflow_runs SET state = ?, state_version = state_version + 1 WHERE id = ? AND state_version = ?",
                ))
                .bind(state.to_string())
                .bind(workflow_run_id)
                .bind(expected_version),
            )
            .await?;
        // no row matched: another writer bumped the version between the caller's read and this
        // write, so the caller's blob is stale and must be rebuilt.
        Ok(result.affected() > 0)
    }

    async fn create_workflow_node_run(
        &self,
        workflow_run_id: Uuid,
        node_id: String,
        parameters: Value,
        prev_node_run_id: Option<Uuid>,
        cursor: Option<&RunCursor>,
    ) -> Result<WorkflowNodeRun, SendableError> {
        let empty_state = Value::Object(Default::default()).to_string();
        let id = Uuid::now_v7();
        let created_at = Utc::now().timestamp();
        // both derived from the one cursor, so they can never disagree about which thread of control
        // produced this attempt.
        let cursor_id = cursor.map(|cursor| cursor.id);
        let speculative = cursor.is_some_and(RunCursor::is_speculative);
        // the origin node run is supplied by the reducer, which knows the true edge taken
        // (including fan-out parents); the database no longer infers it from insertion order.
        if self.dialect() == SqlDialect::MySql {
            let mut conn = self.pool().acquire().await?;
            sqlx::query(&self.render(
                "INSERT INTO workflow_node_runs (id, workflow_run_id, node_id, cursor_id, speculative, status, attempt, parameters, state, prev_node_run_id, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            ))
            .bind(id)
            .bind(workflow_run_id)
            .bind(node_id)
            .bind(cursor_id)
            .bind(speculative)
            .bind(WorkflowStatus::Queued.as_str())
            .bind(0i64)
            .bind(parameters.to_string())
            .bind(empty_state)
            .bind(prev_node_run_id)
            .bind(created_at)
            .execute(&mut *conn)
            .await?;
            let row = sqlx::query(&self.render(&format!(
                "SELECT {WORKFLOW_NODE_RUN_COLUMNS} FROM workflow_node_runs WHERE id = ?"
            )))
            .bind(id)
            .fetch_one(&mut *conn)
            .await?;
            return Ok(mappers::row_to_workflow_node_run(&row));
        }
        let row = sqlx::query(&self.render(&format!(
            "INSERT INTO workflow_node_runs (id, workflow_run_id, node_id, cursor_id, speculative, status, attempt, parameters, state, prev_node_run_id, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             RETURNING {WORKFLOW_NODE_RUN_COLUMNS}",
        )))
        .bind(id)
        .bind(workflow_run_id)
        .bind(node_id)
        .bind(cursor_id)
        .bind(speculative)
        .bind(WorkflowStatus::Queued.as_str())
        .bind(0i64)
        .bind(parameters.to_string())
        .bind(empty_state)
        .bind(prev_node_run_id)
        .bind(created_at)
        .fetch_one(self.pool())
        .await?;
        Ok(mappers::row_to_workflow_node_run(&row))
    }

    async fn update_workflow_node_run(
        &self,
        node_run_id: Uuid,
        status: WorkflowStatus,
        attempt: Option<i64>,
        parameters: Option<Value>,
        output_json: Option<Value>,
        state: Option<Value>,
        transition_reason: Option<String>,
        message: Option<String>,
    ) -> Result<(), SendableError> {
        let now = Utc::now().timestamp();
        let terminal = status.is_terminal();
        self.pool()
            .execute(
                sqlx::query(&self.render(
                    "UPDATE workflow_node_runs SET status = ?, attempt = COALESCE(?, attempt), parameters = COALESCE(?, parameters), output_json = COALESCE(?, output_json), state = COALESCE(?, state), transition_reason = COALESCE(?, transition_reason), message = COALESCE(?, message), started_at = CASE WHEN ? = 'running' THEN ? WHEN ? = 'queued' THEN NULL ELSE started_at END, finished_at = CASE WHEN ? THEN ? WHEN ? = 'queued' THEN NULL ELSE finished_at END WHERE id = ?",
                ))
                .bind(status.as_str())
                .bind(attempt)
                .bind(parameters.map(|value| value.to_string()))
                .bind(output_json.map(|value| value.to_string()))
                .bind(state.map(|value| value.to_string()))
                .bind(transition_reason)
                .bind(message)
                .bind(status.as_str())
                .bind(now)
                .bind(status.as_str())
                .bind(terminal)
                .bind(now)
                .bind(status.as_str())
                .bind(node_run_id),
            )
            .await?;
        Ok(())
    }

    async fn fetch_workflow_node_runs(
        &self,
        workflow_run_id: Uuid,
    ) -> Result<Vec<WorkflowNodeRun>, SendableError> {
        let rows = sqlx::query(&self.render(&format!("SELECT {WORKFLOW_NODE_RUN_COLUMNS} FROM workflow_node_runs WHERE workflow_run_id = ? ORDER BY created_at, id")))
            .bind(workflow_run_id)
            .fetch_all(self.pool())
            .await?;
        Ok(rows.iter().map(mappers::row_to_workflow_node_run).collect())
    }

    async fn release_workflow_node_run_executor(
        &self,
        node_run_id: Uuid,
        replica_id: Uuid,
        released_at: DateTime<Utc>,
    ) -> Result<(), SendableError> {
        // conditional on the holder so a release can only free the caller's own lease: a late or
        // fail-open release from a worker that never held (or no longer holds) the slot must not
        // clear a live claim owned by another replica.
        self.pool()
            .execute(
                sqlx::query(&self.render(
                    "UPDATE workflow_node_runs
                     SET last_executor_replica_id = ?, current_executor_replica_id = NULL, executor_released_at = ?
                     WHERE id = ? AND current_executor_replica_id = ?",
                ))
                .bind(replica_id)
                .bind(released_at.timestamp())
                .bind(node_run_id)
                .bind(replica_id),
            )
            .await?;
        Ok(())
    }

    async fn add_workflow_run_artifact(
        &self,
        artifact: &NewWorkflowRunArtifact,
    ) -> Result<WorkflowRunArtifact, SendableError> {
        let columns = "id, workflow_run_id, node_id, artifact_id, name, mime_type, size_bytes, uri, metadata, created_at";
        let id = Uuid::now_v7();
        let created_at = Utc::now().timestamp();
        if self.dialect() == SqlDialect::MySql {
            let mut conn = self.pool().acquire().await?;
            sqlx::query(&self.render(
                "INSERT INTO workflow_run_artifacts (id, workflow_run_id, node_id, artifact_id, name, mime_type, size_bytes, uri, metadata, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            ))
            .bind(id)
            .bind(artifact.workflow_run_id)
            .bind(artifact.node_id.as_str())
            .bind(artifact.artifact_id)
            .bind(artifact.name.as_str())
            .bind(artifact.mime_type.as_str())
            .bind(artifact.size_bytes)
            .bind(artifact.uri.as_str())
            .bind(artifact.metadata.to_string())
            .bind(created_at)
            .execute(&mut *conn)
            .await?;
            let row = sqlx::query(&self.render(&format!(
                "SELECT {columns} FROM workflow_run_artifacts WHERE id = ?"
            )))
            .bind(id)
            .fetch_one(&mut *conn)
            .await?;
            return Ok(mappers::row_to_workflow_run_artifact(&row));
        }
        let row = sqlx::query(&self.render(&format!(
            "INSERT INTO workflow_run_artifacts (id, workflow_run_id, node_id, artifact_id, name, mime_type, size_bytes, uri, metadata, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             RETURNING {columns}",
        )))
        .bind(id)
        .bind(artifact.workflow_run_id)
        .bind(artifact.node_id.as_str())
        .bind(artifact.artifact_id)
        .bind(artifact.name.as_str())
        .bind(artifact.mime_type.as_str())
        .bind(artifact.size_bytes)
        .bind(artifact.uri.as_str())
        .bind(artifact.metadata.to_string())
        .bind(created_at)
        .fetch_one(self.pool())
        .await?;
        Ok(mappers::row_to_workflow_run_artifact(&row))
    }

    async fn enqueue_ready_node(
        &self,
        event: NewOrchestrationEvent,
        node_id: String,
        ready_at: DateTime<Utc>,
    ) -> Result<Option<ReadyNodeRecord>, SendableError> {
        let mut tx = self.pool().begin().await?;
        let inserted_event = sqlx::query(&self.render(&queries::insert_ignore(
            self.dialect(),
            "workflow_orchestration_events",
            "event_id, workflow_run_id, workflow_node_run_id, node_id, event_type, payload, created_at",
            "?, ?, ?, ?, ?, ?, ?",
            "event_id",
            None,
        )))
        .bind(event.event_id)
        .bind(event.workflow_run_id)
        .bind(event.workflow_node_run_id)
        .bind(event.node_id.clone())
        .bind(event.event_type.as_str())
        .bind(event.payload.to_string())
        .bind(event.created_at.timestamp())
        .execute(&mut *tx)
        .await?;
        if inserted_event.affected() == 0 {
            tx.commit().await?;
            return Ok(None);
        }

        let now = Utc::now().timestamp();

        // invariant: a `(run, node, cursor)` has at most one live pending ready-node generation. every
        // node kind re-arms by enqueuing a fresh row (poll re-checks, timeout wakes, retries,
        // re-entry); if a prior generation's completion is lost — e.g. a db timeout after this new
        // row was already armed — the stale, already-due rows would pile up and feed a runaway that
        // the wake publisher rescans forever. settle any already-due pending rows before adding the
        // new one, so the backlog self-heals to a single live generation. future-dated rows (a node
        // timeout, a not-yet-due poll) are left untouched, and the new row is inserted after, so
        // neither is affected.
        //
        // the cursor scopes it. two threads of control can sit on the same node — a fan-out whose
        // branches converge, or a speculative fork walking beside the branch it came from — and a
        // run-and-node-wide supersede would have each arming silently cancel the other's wake. the
        // predicate is built rather than bound because the three dialects disagree on `? IS NULL`.
        let supersede = match event.cursor_id {
            // legacy untagged rows are superseded too: they are this cursor's own earlier
            // generations, armed before wakes carried a cursor.
            Some(_) => {
                "UPDATE workflow_ready_nodes
                 SET completed_at = ?, status = 'succeeded', updated_at = ?
                 WHERE workflow_run_id = ? AND node_id = ? AND completed_at IS NULL AND ready_at <= ?
                   AND (cursor_id = ? OR cursor_id IS NULL)"
            }
            // an untagged arming keeps the original run-and-node-wide behavior byte for byte.
            None => {
                "UPDATE workflow_ready_nodes
                 SET completed_at = ?, status = 'succeeded', updated_at = ?
                 WHERE workflow_run_id = ? AND node_id = ? AND completed_at IS NULL AND ready_at <= ?"
            }
        };
        let mut supersede = sqlx::query(&self.render(supersede))
            .bind(now)
            .bind(now)
            .bind(event.workflow_run_id)
            .bind(node_id.as_str())
            .bind(now);
        if let Some(cursor_id) = event.cursor_id {
            supersede = supersede.bind(cursor_id);
        }
        supersede.execute(&mut *tx).await?;

        let ready_id = Uuid::now_v7();
        let ready_columns = super::READY_NODE_COLUMNS;

        // mysql has no RETURNING on INSERT IGNORE, so insert then read the row back on the same tx.
        let row = if self.dialect() == SqlDialect::MySql {
            let inserted = sqlx::query(&self.render(&queries::insert_ignore(
                SqlDialect::MySql,
                "workflow_ready_nodes",
                "id, source_event_id, workflow_run_id, node_id, cursor_id, status, ready_at, attempts, created_at, updated_at",
                "?, ?, ?, ?, ?, 'queued', ?, 0, ?, ?",
                "source_event_id, workflow_run_id, node_id",
                None,
            )))
            .bind(ready_id)
            .bind(event.event_id)
            .bind(event.workflow_run_id)
            .bind(node_id.as_str())
            .bind(event.cursor_id)
            .bind(ready_at.timestamp())
            .bind(now)
            .bind(now)
            .execute(&mut *tx)
            .await?;
            if inserted.affected() == 0 {
                None
            } else {
                Some(
                    sqlx::query(&self.render(&format!(
                        "SELECT {ready_columns} FROM workflow_ready_nodes WHERE source_event_id = ? AND workflow_run_id = ? AND node_id = ?",
                    )))
                    .bind(event.event_id)
                    .bind(event.workflow_run_id)
                    .bind(node_id.as_str())
                    .fetch_one(&mut *tx)
                    .await?,
                )
            }
        } else {
            sqlx::query(&self.render(&queries::insert_ignore(
                self.dialect(),
                "workflow_ready_nodes",
                "id, source_event_id, workflow_run_id, node_id, cursor_id, status, ready_at, attempts, created_at, updated_at",
                "?, ?, ?, ?, ?, 'queued', ?, 0, ?, ?",
                "source_event_id, workflow_run_id, node_id",
                Some(ready_columns),
            )))
            .bind(ready_id)
            .bind(event.event_id)
            .bind(event.workflow_run_id)
            .bind(node_id.as_str())
            .bind(event.cursor_id)
            .bind(ready_at.timestamp())
            .bind(now)
            .bind(now)
            .fetch_optional(&mut *tx)
            .await?
        };
        tx.commit().await?;
        row.as_ref().map(mappers::row_to_ready_node).transpose()
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

    async fn enqueue_action_dispatch(
        &self,
        dedupe_key: String,
        command: ActionCommand,
    ) -> Result<ActionDispatchRecord, SendableError> {
        let now = Utc::now().timestamp();
        let id = Uuid::now_v7();
        let dispatch_columns = "id, dedupe_key, command_json, attempts, created_at, updated_at, published_at, last_error, claimed_by, claimed_until";

        // first writer wins: keep the existing command on conflict.
        if self.dialect() == SqlDialect::MySql {
            sqlx::query(&self.render(
                "INSERT INTO workflow_action_dispatches (id, dedupe_key, command_json, attempts, created_at, updated_at)
                 VALUES (?, ?, ?, 0, ?, ?)
                 ON DUPLICATE KEY UPDATE command_json = command_json",
            ))
            .bind(id)
            .bind(dedupe_key.as_str())
            .bind(serde_json::to_string(&command)?)
            .bind(now)
            .bind(now)
            .execute(self.pool())
            .await?;
            let row = sqlx::query(&self.render(&format!(
                "SELECT {dispatch_columns} FROM workflow_action_dispatches WHERE dedupe_key = ?",
            )))
            .bind(dedupe_key)
            .fetch_one(self.pool())
            .await?;
            return mappers::row_to_action_dispatch(&row);
        }

        let row = sqlx::query(&self.render(&format!(
            "INSERT INTO workflow_action_dispatches (id, dedupe_key, command_json, attempts, created_at, updated_at)
             VALUES (?, ?, ?, 0, ?, ?)
             ON CONFLICT(dedupe_key) DO UPDATE SET command_json = workflow_action_dispatches.command_json
             RETURNING {dispatch_columns}",
        )))
        .bind(id)
        .bind(dedupe_key)
        .bind(serde_json::to_string(&command)?)
        .bind(now)
        .bind(now)
        .fetch_one(self.pool())
        .await?;
        mappers::row_to_action_dispatch(&row)
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
        let firing_sql = self.render(&queries::insert_ignore(
            self.dialect(),
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
        Ok(rows.iter().map(mappers::row_to_workflow_run).collect())
    }

    async fn try_record_trigger_firing(
        &self,
        trigger_id: Uuid,
        fire_key: String,
    ) -> Result<bool, SendableError> {
        // insert-ignore on the unique (trigger_id, fire_key); a zero-row insert means another
        // caller already recorded this firing, so the caller must not start a duplicate run.
        let firing_sql = self.render(&queries::insert_ignore(
            self.dialect(),
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
        let id = Uuid::now_v7();
        let created_at = Utc::now().timestamp();
        if self.dialect() == SqlDialect::MySql {
            let mut conn = self.pool().acquire().await?;
            sqlx::query(&self.render(
                "INSERT INTO workflow_runs (id, workflow_id, workflow_snapshot, status, active_node_id, parameters, state, created_at, name, trigger_source_kind, trigger_actor_type, trigger_actor_replica_id, trigger_actor_display_name, trigger_request_host, trigger_request_ip, trigger_metadata) VALUES (?, ?, ?, ?, NULL, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            ))
            .bind(id)
            .bind(workflow_id)
            .bind(snapshot)
            .bind(WorkflowStatus::Queued.as_str())
            .bind(parameters.to_string())
            .bind(state.to_string())
            .bind(created_at)
            .bind(name)
            .bind(provenance.source_kind.map(|value| value.as_str().to_string()))
            .bind(provenance.actor_type.map(|value| value.as_str().to_string()))
            .bind(provenance.actor_replica_id)
            .bind(provenance.actor_display_name)
            .bind(provenance.request_host)
            .bind(provenance.request_ip)
            .bind(provenance.metadata.to_string())
            .execute(&mut *conn)
            .await?;
            let row = sqlx::query(&self.render(&format!(
                "SELECT {WORKFLOW_RUN_COLUMNS} FROM workflow_runs WHERE id = ?"
            )))
            .bind(id)
            .fetch_one(&mut *conn)
            .await?;
            return Ok(mappers::row_to_workflow_run(&row));
        }
        let row = sqlx::query(&self.render(&format!(
            "INSERT INTO workflow_runs (id, workflow_id, workflow_snapshot, status, active_node_id, parameters, state, created_at, name, trigger_source_kind, trigger_actor_type, trigger_actor_replica_id, trigger_actor_display_name, trigger_request_host, trigger_request_ip, trigger_metadata)
             VALUES (?, ?, ?, ?, NULL, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             RETURNING {WORKFLOW_RUN_COLUMNS}",
        )))
        .bind(id)
        .bind(workflow_id)
        .bind(snapshot)
        .bind(WorkflowStatus::Queued.as_str())
        .bind(parameters.to_string())
        .bind(state.to_string())
        .bind(created_at)
        .bind(name)
        .bind(provenance.source_kind.map(|value| value.as_str().to_string()))
        .bind(provenance.actor_type.map(|value| value.as_str().to_string()))
        .bind(provenance.actor_replica_id)
        .bind(provenance.actor_display_name)
        .bind(provenance.request_host)
        .bind(provenance.request_ip)
        .bind(provenance.metadata.to_string())
        .fetch_one(self.pool())
        .await?;
        Ok(mappers::row_to_workflow_run(&row))
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
        Ok(rows.iter().map(mappers::row_to_workflow_run).collect())
    }

    async fn fetch_workflow_node_runs_by_status(
        &self,
        status: WorkflowStatus,
    ) -> Result<Vec<WorkflowNodeRun>, SendableError> {
        let rows = sqlx::query(&self.render(&format!(
            "SELECT {WORKFLOW_NODE_RUN_COLUMNS} FROM workflow_node_runs WHERE status = ? ORDER BY created_at, id"
        )))
        .bind(status.as_str())
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_workflow_node_run).collect())
    }

    async fn fetch_workflow_node_run_artifacts_for_run(
        &self,
        workflow_run_id: Uuid,
    ) -> Result<Vec<WorkflowNodeRunArtifact>, SendableError> {
        let rows = sqlx::query(&self.render(
            "SELECT a.id, a.workflow_node_run_id, a.name, a.mime_type, a.size_bytes, a.uri, a.metadata, a.created_at
             FROM workflow_node_artifacts a
             JOIN workflow_node_runs r ON a.workflow_node_run_id = r.id
             WHERE r.workflow_run_id = ?
             ORDER BY a.created_at ASC, a.id ASC",
        ))
        .bind(workflow_run_id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows
            .iter()
            .map(mappers::row_to_workflow_node_run_artifact)
            .collect())
    }

    async fn fetch_replicas(
        &self,
        replica_type: Option<ReplicaKind>,
        status: Option<ReplicaStatus>,
        stale_before: DateTime<Utc>,
    ) -> Result<Vec<ReplicaRecord>, SendableError> {
        let rows = if let Some(replica_type) = replica_type {
            sqlx::query(&self.render(&format!(
                "SELECT replica_id, replica_type, instance_id, runtime_id,
                        CASE
                            WHEN status = 'offline' THEN 'offline'
                            WHEN last_heartbeat_at <= ? THEN 'stale'
                            ELSE 'live'
                        END AS status,
                        display_name, host, port, base_path, observed_ip, version, attributes, first_seen_at, last_heartbeat_at, last_seen_at, offline_at,
                        registered_by_principal_id, registered_by_kind, registered_by_org_id
                 FROM replicas WHERE replica_type = ? ORDER BY replica_type, instance_id, replica_id"
            )))
            .bind(stale_before.timestamp())
            .bind(replica_type.as_str())
            .fetch_all(self.pool())
            .await?
        } else {
            sqlx::query(&self.render(
                "SELECT replica_id, replica_type, instance_id, runtime_id,
                        CASE
                            WHEN status = 'offline' THEN 'offline'
                            WHEN last_heartbeat_at <= ? THEN 'stale'
                            ELSE 'live'
                        END AS status,
                        display_name, host, port, base_path, observed_ip, version, attributes, first_seen_at, last_heartbeat_at, last_seen_at, offline_at,
                        registered_by_principal_id, registered_by_kind, registered_by_org_id
                 FROM replicas ORDER BY replica_type, instance_id, replica_id",
            ))
            .bind(stale_before.timestamp())
            .fetch_all(self.pool())
            .await?
        };
        let mut replicas = rows
            .iter()
            .map(mappers::row_to_replica)
            .collect::<Result<Vec<_>, _>>()?;
        if let Some(status) = status {
            replicas.retain(|replica| replica.status == status);
        }
        Ok(replicas)
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
