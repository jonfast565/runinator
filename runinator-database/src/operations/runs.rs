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
        if self.dialect() == SqlDialect::MySql {
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

    async fn claim_workflow_node_run_executor(
        &self,
        node_run_id: Uuid,
        replica_id: Uuid,
        claimed_at: DateTime<Utc>,
        stale_before: DateTime<Utc>,
        heartbeat_stale_before: DateTime<Utc>,
    ) -> Result<bool, SendableError> {
        // compare-and-swap lease: only acquire when no live executor holds the slot. a redelivered
        // or timeout-raced duplicate of the same node run thus cannot execute concurrently. the slot
        // frees on release, once the prior claim ages past `stale_before` (the caller's deadline), or
        // as soon as the holder stops being live. the liveness arm is the fast path: a crashed worker
        // is detected by its missing heartbeat rather than by the action's timeout, so failover no
        // longer waits out a long job's whole deadline. a holder that shut down gracefully is already
        // marked offline and frees the slot on the next claim.
        let result = self
            .pool()
            .execute(
                sqlx::query(&self.render(
                    "UPDATE workflow_node_runs
                     SET current_executor_replica_id = ?, executor_claimed_at = ?, executor_released_at = NULL
                     WHERE id = ?
                       AND (
                         current_executor_replica_id IS NULL
                         OR executor_claimed_at < ?
                         OR NOT EXISTS (
                           SELECT 1 FROM replicas r
                           WHERE r.replica_id = workflow_node_runs.current_executor_replica_id
                             AND r.status <> 'offline'
                             AND r.last_heartbeat_at >= ?
                         )
                       )",
                ))
                .bind(replica_id)
                .bind(claimed_at.timestamp())
                .bind(node_run_id)
                .bind(stale_before.timestamp())
                .bind(heartbeat_stale_before.timestamp()),
            )
            .await?;
        Ok(result.affected() > 0)
    }

    async fn fetch_workflow_node_run(
        &self,
        workflow_node_run_id: Uuid,
    ) -> Result<Option<WorkflowNodeRun>, SendableError> {
        let row = sqlx::query(&self.render(&format!(
            "SELECT {WORKFLOW_NODE_RUN_COLUMNS} FROM workflow_node_runs WHERE id = ?"
        )))
        .bind(workflow_node_run_id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|row| mappers::row_to_workflow_node_run(&row)))
    }

    async fn append_workflow_node_run_chunk(
        &self,
        workflow_node_run_id: Uuid,
        chunk: &NewRunChunk,
    ) -> Result<WorkflowNodeRunChunk, SendableError> {
        let sequence: i64 = sqlx::query(&self.render("SELECT COALESCE(MAX(sequence), 0) + 1 AS next_sequence FROM workflow_node_chunks WHERE workflow_node_run_id = ?"))
            .bind(workflow_node_run_id)
            .fetch_one(self.pool())
            .await?
            .get("next_sequence");
        let columns = "id, workflow_node_run_id, sequence, stream, content, created_at";
        let id = Uuid::now_v7();
        let created_at = Utc::now().timestamp();
        if self.dialect() == SqlDialect::MySql {
            let mut conn = self.pool().acquire().await?;
            sqlx::query(&self.render(
                "INSERT INTO workflow_node_chunks (id, workflow_node_run_id, sequence, stream, content, created_at) VALUES (?, ?, ?, ?, ?, ?)",
            ))
            .bind(id)
            .bind(workflow_node_run_id)
            .bind(sequence)
            .bind(chunk.stream.as_str())
            .bind(chunk.content.as_str())
            .bind(created_at)
            .execute(&mut *conn)
            .await?;
            let row = sqlx::query(&self.render(&format!(
                "SELECT {columns} FROM workflow_node_chunks WHERE id = ?"
            )))
            .bind(id)
            .fetch_one(&mut *conn)
            .await?;
            return Ok(mappers::row_to_workflow_node_run_chunk(&row));
        }
        let row = sqlx::query(&self.render(&format!(
            "INSERT INTO workflow_node_chunks (id, workflow_node_run_id, sequence, stream, content, created_at)
             VALUES (?, ?, ?, ?, ?, ?)
             RETURNING {columns}",
        )))
        .bind(id)
        .bind(workflow_node_run_id)
        .bind(sequence)
        .bind(chunk.stream.as_str())
        .bind(chunk.content.as_str())
        .bind(created_at)
        .fetch_one(self.pool())
        .await?;
        Ok(mappers::row_to_workflow_node_run_chunk(&row))
    }

    async fn fetch_workflow_node_run_chunks(
        &self,
        workflow_node_run_id: Uuid,
        cursor: Option<i64>,
        limit: i64,
    ) -> Result<Vec<WorkflowNodeRunChunk>, SendableError> {
        let rows = sqlx::query(&self.render(
            "SELECT id, workflow_node_run_id, sequence, stream, content, created_at FROM workflow_node_chunks WHERE workflow_node_run_id = ? AND sequence > ? ORDER BY sequence ASC LIMIT ?",
        ))
        .bind(workflow_node_run_id)
        .bind(cursor.unwrap_or(0))
        .bind(limit.clamp(1, 1000))
        .fetch_all(self.pool())
        .await?;
        Ok(rows
            .iter()
            .map(mappers::row_to_workflow_node_run_chunk)
            .collect())
    }

    async fn add_workflow_node_run_artifact(
        &self,
        workflow_node_run_id: Uuid,
        artifact: &NewRunArtifact,
    ) -> Result<WorkflowNodeRunArtifact, SendableError> {
        let columns =
            "id, workflow_node_run_id, name, mime_type, size_bytes, uri, metadata, created_at";
        let id = Uuid::now_v7();
        let created_at = Utc::now().timestamp();
        if self.dialect() == SqlDialect::MySql {
            let mut conn = self.pool().acquire().await?;
            sqlx::query(&self.render(
                "INSERT INTO workflow_node_artifacts (id, workflow_node_run_id, name, mime_type, size_bytes, uri, metadata, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            ))
            .bind(id)
            .bind(workflow_node_run_id)
            .bind(artifact.name.as_str())
            .bind(artifact.mime_type.as_str())
            .bind(artifact.size_bytes)
            .bind(artifact.uri.as_str())
            .bind(artifact.metadata.to_string())
            .bind(created_at)
            .execute(&mut *conn)
            .await?;
            let row = sqlx::query(&self.render(&format!(
                "SELECT {columns} FROM workflow_node_artifacts WHERE id = ?"
            )))
            .bind(id)
            .fetch_one(&mut *conn)
            .await?;
            return Ok(mappers::row_to_workflow_node_run_artifact(&row));
        }
        let row = sqlx::query(&self.render(&format!(
            "INSERT INTO workflow_node_artifacts (id, workflow_node_run_id, name, mime_type, size_bytes, uri, metadata, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             RETURNING {columns}",
        )))
        .bind(id)
        .bind(workflow_node_run_id)
        .bind(artifact.name.as_str())
        .bind(artifact.mime_type.as_str())
        .bind(artifact.size_bytes)
        .bind(artifact.uri.as_str())
        .bind(artifact.metadata.to_string())
        .bind(created_at)
        .fetch_one(self.pool())
        .await?;
        Ok(mappers::row_to_workflow_node_run_artifact(&row))
    }

    async fn fetch_workflow_node_run_artifacts(
        &self,
        workflow_node_run_id: Uuid,
    ) -> Result<Vec<WorkflowNodeRunArtifact>, SendableError> {
        let rows = sqlx::query(&self.render(
            "SELECT id, workflow_node_run_id, name, mime_type, size_bytes, uri, metadata, created_at FROM workflow_node_artifacts WHERE workflow_node_run_id = ? ORDER BY created_at ASC, id ASC",
        ))
        .bind(workflow_node_run_id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows
            .iter()
            .map(mappers::row_to_workflow_node_run_artifact)
            .collect())
    }

    async fn fetch_workflow_run_artifacts(
        &self,
        workflow_run_id: Uuid,
    ) -> Result<Vec<WorkflowRunArtifact>, SendableError> {
        let rows = sqlx::query(&self.render(
            "SELECT id, workflow_run_id, node_id, artifact_id, name, mime_type, size_bytes, uri, metadata, created_at FROM workflow_run_artifacts WHERE workflow_run_id = ? ORDER BY created_at ASC, id ASC",
        ))
        .bind(workflow_run_id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows
            .iter()
            .map(mappers::row_to_workflow_run_artifact)
            .collect())
    }

    async fn apply_workflow_result_event(
        &self,
        event: &WorkflowResultEvent,
    ) -> Result<bool, SendableError> {
        let mut tx = self.pool().begin().await?;
        let event_type = workflow_result_event_type(event);
        let insert = sqlx::query(&self.render(&self.dialect().insert_ignore(
            "workflow_result_events",
            "event_id, workflow_run_id, workflow_node_run_id, node_id, event_type, created_at",
            "?, ?, ?, ?, ?, ?",
            "event_id",
            None,
        )))
        .bind(event.event_id)
        .bind(event.workflow_run_id)
        .bind(event.workflow_node_run_id)
        .bind(event.node_id.clone())
        .bind(event_type)
        .bind(event.timestamp.timestamp())
        .execute(&mut *tx)
        .await?;

        if insert.affected() == 0 {
            tx.commit().await?;
            return Ok(false);
        }

        match &event.kind {
            WorkflowResultEventKind::Status {
                status,
                output_json,
                message,
            } => {
                let now = Utc::now().timestamp();
                let terminal = status.is_terminal();
                // the trailing attempt guard discards a very late result from a superseded attempt
                // (the row's attempt has moved past the event's), which would otherwise overwrite
                // the retry's status. attempt 0 marks an older message with no attempt: apply it
                // unconditionally as before.
                sqlx::query(&self.render(
                    "UPDATE workflow_node_runs SET status = ?, output_json = COALESCE(?, output_json), message = COALESCE(?, message), started_at = CASE WHEN ? = 'running' THEN ? WHEN ? = 'queued' THEN NULL ELSE started_at END, finished_at = CASE WHEN ? THEN ? WHEN ? = 'queued' THEN NULL ELSE finished_at END WHERE id = ? AND NOT (status IN ('succeeded', 'failed', 'timed_out', 'canceled') AND ? NOT IN ('succeeded', 'failed', 'timed_out', 'canceled')) AND (? <= 0 OR attempt <= ?)",
                ))
                .bind(status.as_str())
                .bind(output_json.as_ref().map(|value: &Value| value.to_string()))
                .bind(message.clone())
                .bind(status.as_str())
                .bind(now)
                .bind(status.as_str())
                .bind(terminal)
                .bind(now)
                .bind(status.as_str())
                .bind(event.workflow_node_run_id)
                .bind(status.as_str())
                .bind(event.attempt)
                .bind(event.attempt)
                .execute(&mut *tx)
                .await?;
            }
            WorkflowResultEventKind::Chunk { chunk } => {
                let sequence: i64 = sqlx::query(&self.render("SELECT COALESCE(MAX(sequence), 0) + 1 AS next_sequence FROM workflow_node_chunks WHERE workflow_node_run_id = ?"))
                    .bind(event.workflow_node_run_id)
                    .fetch_one(&mut *tx)
                    .await?
                    .get("next_sequence");
                sqlx::query(&self.render(
                    "INSERT INTO workflow_node_chunks (id, workflow_node_run_id, sequence, stream, content, created_at)
                     VALUES (?, ?, ?, ?, ?, ?)",
                ))
                .bind(Uuid::now_v7())
                .bind(event.workflow_node_run_id)
                .bind(sequence)
                .bind(chunk.stream.as_str())
                .bind(chunk.content.as_str())
                .bind(event.timestamp.timestamp())
                .execute(&mut *tx)
                .await?;
            }
            WorkflowResultEventKind::Artifact { artifact } => {
                sqlx::query(&self.render(
                    "INSERT INTO workflow_node_artifacts (id, workflow_node_run_id, name, mime_type, size_bytes, uri, metadata, created_at)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                ))
                .bind(Uuid::now_v7())
                .bind(event.workflow_node_run_id)
                .bind(artifact.name.as_str())
                .bind(artifact.mime_type.as_str())
                .bind(artifact.size_bytes)
                .bind(artifact.uri.as_str())
                .bind(artifact.metadata.to_string())
                .bind(event.timestamp.timestamp())
                .execute(&mut *tx)
                .await?;
            }
        }

        tx.commit().await?;
        Ok(true)
    }

    async fn append_orchestration_event(
        &self,
        event: &NewOrchestrationEvent,
    ) -> Result<bool, SendableError> {
        let insert = sqlx::query(&self.render(&self.dialect().insert_ignore(
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
        .execute(self.pool())
        .await?;
        Ok(insert.affected() > 0)
    }

    async fn fetch_orchestration_events(
        &self,
        workflow_run_id: Uuid,
        limit: i64,
    ) -> Result<Vec<OrchestrationEvent>, SendableError> {
        let rows = sqlx::query(&self.render(
            "SELECT event_id, workflow_run_id, workflow_node_run_id, node_id, event_type, payload, created_at
             FROM workflow_orchestration_events
             WHERE workflow_run_id = ?
             ORDER BY created_at, event_id
             LIMIT ?",
        ))
        .bind(workflow_run_id)
        .bind(limit.max(1))
        .fetch_all(self.pool())
        .await?;
        rows.iter()
            .map(mappers::row_to_orchestration_event)
            .collect()
    }

    async fn fetch_run_transitions(
        &self,
        workflow_run_id: Uuid,
    ) -> Result<Vec<NodeTransition>, SendableError> {
        // reconstruct edges from the node-run chain, ordered as the run walked them. the reason an
        // edge was taken is the predecessor's transition_reason (why it left toward this node).
        let node_runs = self.fetch_workflow_node_runs(workflow_run_id).await?;
        let by_id: HashMap<Uuid, &WorkflowNodeRun> =
            node_runs.iter().map(|run| (run.id, run)).collect();
        let transitions = node_runs
            .iter()
            .map(|run| {
                let prev = run.prev_node_run_id.and_then(|id| by_id.get(&id));
                NodeTransition {
                    from_node: prev.map(|p| p.node_id.clone()),
                    to_node: run.node_id.clone(),
                    reason: prev.and_then(|p| p.transition_reason.clone()),
                    node_run_id: run.id,
                    at: run.created_at,
                }
            })
            .collect();
        Ok(transitions)
    }

    async fn fetch_node_transition_stats(
        &self,
        workflow_id: Uuid,
        node_id: Option<String>,
    ) -> Result<Vec<NodeTransitionStat>, SendableError> {
        // pull every walked edge for the workflow, then aggregate in rust so last_reason stays exact
        // and the query stays dialect-neutral (no window functions).
        let mut sql = String::from(
            "SELECT prev.node_id AS from_node, cur.node_id AS to_node, \
                    prev.transition_reason AS reason, cur.created_at AS at \
             FROM workflow_node_runs cur \
             JOIN workflow_node_runs prev ON cur.prev_node_run_id = prev.id \
             JOIN workflow_runs r ON cur.workflow_run_id = r.id \
             WHERE r.workflow_id = ?",
        );
        if node_id.is_some() {
            sql.push_str(" AND prev.node_id = ?");
        }
        let rendered = self.render(&sql);
        let mut query = sqlx::query(&rendered).bind(workflow_id);
        if let Some(node_id) = node_id.as_ref() {
            query = query.bind(node_id);
        }
        let rows = query.fetch_all(self.pool()).await?;
        let mut stats: HashMap<(String, String), NodeTransitionStat> = HashMap::new();
        for row in &rows {
            let from_node: String = row.get("from_node");
            let to_node: String = row.get("to_node");
            let reason: Option<String> = row.get("reason");
            let at = DateTime::<Utc>::from_timestamp(row.get::<i64, _>("at"), 0)
                .unwrap_or_else(Utc::now);
            let entry = stats
                .entry((from_node.clone(), to_node.clone()))
                .or_insert_with(|| NodeTransitionStat {
                    from_node,
                    to_node,
                    count: 0,
                    last_reason: None,
                    last_at: at,
                });
            entry.count += 1;
            if at >= entry.last_at {
                entry.last_at = at;
                entry.last_reason = reason;
            }
        }
        Ok(stats.into_values().collect())
    }

    async fn claim_ready_nodes(
        &self,
        scheduler_id: String,
        now: DateTime<Utc>,
        lease_until: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<ReadyNodeRecord>, SendableError> {
        let columns = super::READY_NODE_COLUMNS;

        // mysql has no UPDATE ... RETURNING and cannot subquery the table being updated, so claim
        // via a derived-table subselect, then read the claimed rows back by the lease just written.
        if self.dialect() == SqlDialect::MySql {
            sqlx::query(&self.render(
                "UPDATE workflow_ready_nodes
                 SET claimed_by = ?, claimed_until = ?, attempts = attempts + 1, status = 'running', updated_at = ?
                 WHERE id IN (
                     SELECT id FROM (
                         SELECT id FROM workflow_ready_nodes
                         WHERE completed_at IS NULL
                           AND ready_at <= ?
                           AND (claimed_until IS NULL OR claimed_until <= ? OR claimed_by = ?)
                         ORDER BY ready_at, id
                         LIMIT ?
                     ) AS claimable
                 )",
            ))
            .bind(scheduler_id.as_str())
            .bind(lease_until.timestamp())
            .bind(now.timestamp())
            .bind(now.timestamp())
            .bind(now.timestamp())
            .bind(scheduler_id.as_str())
            .bind(limit.max(1))
            .execute(self.pool())
            .await?;
            let rows = sqlx::query(&self.render(&format!(
                "SELECT {columns} FROM workflow_ready_nodes WHERE claimed_by = ? AND claimed_until = ? ORDER BY ready_at, id",
            )))
            .bind(scheduler_id.as_str())
            .bind(lease_until.timestamp())
            .fetch_all(self.pool())
            .await?;
            return rows.iter().map(mappers::row_to_ready_node).collect();
        }

        let sql = self.render(&format!(
            "UPDATE workflow_ready_nodes
             SET claimed_by = ?, claimed_until = ?, attempts = attempts + 1, status = 'running', updated_at = ?
             WHERE id IN (
                 SELECT id FROM workflow_ready_nodes
                 WHERE completed_at IS NULL
                   AND ready_at <= ?
                   AND (claimed_until IS NULL OR claimed_until <= ? OR claimed_by = ?)
                 ORDER BY ready_at, id
                 LIMIT ?{skip}
             )
             RETURNING {columns}",
            skip = self.dialect().skip_locked(),
        ));
        let rows = sqlx::query(&sql)
            .bind(scheduler_id.as_str())
            .bind(lease_until.timestamp())
            .bind(now.timestamp())
            .bind(now.timestamp())
            .bind(now.timestamp())
            .bind(scheduler_id.as_str())
            .bind(limit.max(1))
            .fetch_all(self.pool())
            .await?;
        rows.iter().map(mappers::row_to_ready_node).collect()
    }

    async fn fetch_ready_node(
        &self,
        ready_node_id: Uuid,
    ) -> Result<Option<ReadyNodeRecord>, SendableError> {
        let row = sqlx::query(&self.render(&format!(
            "SELECT {}
             FROM workflow_ready_nodes
             WHERE id = ?",
            super::READY_NODE_COLUMNS
        )))
        .bind(ready_node_id)
        .fetch_optional(self.pool())
        .await?;
        row.as_ref().map(mappers::row_to_ready_node).transpose()
    }

    async fn complete_ready_node(
        &self,
        ready_node_id: Uuid,
        scheduler_id: String,
    ) -> Result<bool, SendableError> {
        let now = Utc::now().timestamp();
        let result = sqlx::query(&self.render(
            "UPDATE workflow_ready_nodes
             SET completed_at = ?, status = 'succeeded', updated_at = ?
             WHERE id = ? AND claimed_by = ?",
        ))
        .bind(now)
        .bind(now)
        .bind(ready_node_id)
        .bind(scheduler_id)
        .execute(self.pool())
        .await?;
        Ok(result.affected() > 0)
    }

    async fn fetch_pending_ready_nodes(
        &self,
        now: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<ReadyNodeRecord>, SendableError> {
        let rows = sqlx::query(&self.render(&format!(
            "SELECT {}
             FROM workflow_ready_nodes
             WHERE completed_at IS NULL
               AND (claimed_until IS NULL OR claimed_until <= ?)
             ORDER BY ready_at, id
             LIMIT ?",
            super::READY_NODE_COLUMNS
        )))
        .bind(now.timestamp())
        .bind(limit.max(1))
        .fetch_all(self.pool())
        .await?;
        rows.iter().map(mappers::row_to_ready_node).collect()
    }

    async fn claim_ready_nodes_for_announce(
        &self,
        now: DateTime<Utc>,
        lease_seconds: i64,
        limit: i64,
    ) -> Result<Vec<ReadyNodeRecord>, SendableError> {
        let columns = super::READY_NODE_COLUMNS;
        let now_ts = now.timestamp();
        let lease = lease_seconds.max(1);

        // mysql has no UPDATE ... RETURNING, so it stamps a uniform `now + lease` and reads the
        // rows back by that marker; future-dated rows are re-announced once per window there, a
        // bounded duplication the waker tolerates.
        if self.dialect() == SqlDialect::MySql {
            let lease_ts = now_ts + lease;
            sqlx::query(&self.render(
                "UPDATE workflow_ready_nodes
                 SET announced_until = ?, updated_at = ?
                 WHERE id IN (
                     SELECT id FROM (
                         SELECT id FROM workflow_ready_nodes
                         WHERE completed_at IS NULL
                           AND (claimed_until IS NULL OR claimed_until <= ?)
                           AND (announced_until IS NULL OR announced_until <= ?)
                         ORDER BY ready_at, id
                         LIMIT ?
                     ) AS announceable
                 )",
            ))
            .bind(lease_ts)
            .bind(now_ts)
            .bind(now_ts)
            .bind(now_ts)
            .bind(limit.max(1))
            .execute(self.pool())
            .await?;
            let rows = sqlx::query(&self.render(&format!(
                "SELECT {columns} FROM workflow_ready_nodes WHERE announced_until = ? AND completed_at IS NULL ORDER BY ready_at, id",
            )))
            .bind(lease_ts)
            .fetch_all(self.pool())
            .await?;
            return rows.iter().map(mappers::row_to_ready_node).collect();
        }

        // stamp the lease past each row's ready_at so a not-yet-due row is announced once and the
        // waker alone owns its timer; the lease only lapses (and re-announces) after the row is due.
        let sql = self.render(&format!(
            "UPDATE workflow_ready_nodes
             SET announced_until = (CASE WHEN ready_at > ? THEN ready_at ELSE ? END) + ?, updated_at = ?
             WHERE id IN (
                 SELECT id FROM workflow_ready_nodes
                 WHERE completed_at IS NULL
                   AND (claimed_until IS NULL OR claimed_until <= ?)
                   AND (announced_until IS NULL OR announced_until <= ?)
                 ORDER BY ready_at, id
                 LIMIT ?{skip}
             )
             RETURNING {columns}",
            skip = self.dialect().skip_locked(),
        ));
        let rows = sqlx::query(&sql)
            .bind(now_ts)
            .bind(now_ts)
            .bind(lease)
            .bind(now_ts)
            .bind(now_ts)
            .bind(now_ts)
            .bind(limit.max(1))
            .fetch_all(self.pool())
            .await?;
        rows.iter().map(mappers::row_to_ready_node).collect()
    }

    async fn claim_ready_node(
        &self,
        ready_node_id: Uuid,
        scheduler_id: String,
        now: DateTime<Utc>,
        lease_until: DateTime<Utc>,
    ) -> Result<Option<ReadyNodeRecord>, SendableError> {
        let columns = super::READY_NODE_COLUMNS;

        // mysql has no UPDATE ... RETURNING: claim by id, then read back only if we hold the lease.
        if self.dialect() == SqlDialect::MySql {
            sqlx::query(&self.render(
                "UPDATE workflow_ready_nodes
                 SET claimed_by = ?, claimed_until = ?, attempts = attempts + 1, status = 'running', updated_at = ?
                 WHERE id = ?
                   AND completed_at IS NULL
                   AND (claimed_until IS NULL OR claimed_until <= ? OR claimed_by = ?)",
            ))
            .bind(scheduler_id.as_str())
            .bind(lease_until.timestamp())
            .bind(now.timestamp())
            .bind(ready_node_id)
            .bind(now.timestamp())
            .bind(scheduler_id.as_str())
            .execute(self.pool())
            .await?;
            let row = sqlx::query(&self.render(&format!(
                "SELECT {columns} FROM workflow_ready_nodes WHERE id = ? AND claimed_by = ? AND claimed_until = ?",
            )))
            .bind(ready_node_id)
            .bind(scheduler_id.as_str())
            .bind(lease_until.timestamp())
            .fetch_optional(self.pool())
            .await?;
            return row.as_ref().map(mappers::row_to_ready_node).transpose();
        }

        let row = sqlx::query(&self.render(&format!(
            "UPDATE workflow_ready_nodes
             SET claimed_by = ?, claimed_until = ?, attempts = attempts + 1, status = 'running', updated_at = ?
             WHERE id = ?
               AND completed_at IS NULL
               AND (claimed_until IS NULL OR claimed_until <= ? OR claimed_by = ?)
             RETURNING {columns}",
        )))
        .bind(scheduler_id.as_str())
        .bind(lease_until.timestamp())
        .bind(now.timestamp())
        .bind(ready_node_id)
        .bind(now.timestamp())
        .bind(scheduler_id.as_str())
        .fetch_optional(self.pool())
        .await?;
        row.as_ref().map(mappers::row_to_ready_node).transpose()
    }

    async fn release_ready_node(
        &self,
        ready_node_id: Uuid,
        scheduler_id: String,
    ) -> Result<bool, SendableError> {
        let now = Utc::now().timestamp();
        let result = sqlx::query(&self.render(
            "UPDATE workflow_ready_nodes
             SET claimed_by = NULL, claimed_until = NULL, status = 'queued', updated_at = ?
             WHERE id = ? AND claimed_by = ? AND completed_at IS NULL",
        ))
        .bind(now)
        .bind(ready_node_id)
        .bind(scheduler_id)
        .execute(self.pool())
        .await?;
        Ok(result.affected() > 0)
    }

    async fn settle_terminal_run_ready_nodes(&self, limit: i64) -> Result<u64, SendableError> {
        let now = Utc::now().timestamp();
        // the derived-table wrapper lets mysql update a table it also selects from; sqlite/postgres
        // accept it too, so one statement serves every dialect. the join is index-backed on
        // workflow_run_id / status.
        let result = sqlx::query(&self.render(
            "UPDATE workflow_ready_nodes
             SET completed_at = ?, status = 'succeeded', updated_at = ?
             WHERE id IN (
                 SELECT id FROM (
                     SELECT rn.id FROM workflow_ready_nodes rn
                     JOIN workflow_runs wr ON wr.id = rn.workflow_run_id
                     WHERE rn.completed_at IS NULL
                       AND wr.status IN ('succeeded', 'failed', 'timed_out', 'canceled')
                     LIMIT ?
                 ) AS doomed
             )",
        ))
        .bind(now)
        .bind(now)
        .bind(limit.max(1))
        .execute(self.pool())
        .await?;
        Ok(result.affected())
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
