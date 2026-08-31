//! workflow/pipeline definitions and the provider catalog.
//!
//! the `DefinitionStore` half of the generic sql implementation. bodies are written once, over any
//! `SqlBackend`; see `super` for the shared helpers they call.

use super::*;

// the bound list is repeated verbatim in every role impl in this directory. it stays spelled out
// rather than hidden behind a macro so that type errors inside the query bodies — the part that
// actually gets edited — keep pointing at real source lines instead of a macro expansion.
impl<B> DefinitionStore for SqlStore<B>
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
    async fn upsert_workflow(
        &self,
        workflow: &WorkflowDefinition,
    ) -> Result<WorkflowDefinition, SendableError> {
        let now = Utc::now().timestamp();
        // An explicit stable key identifies the logical workflow across display-name edits and
        // namespace moves. Legacy callers without one keep the old path lookup until migration.
        let existing_id = match workflow.id {
            Some(id) => Some(id),
            None if workflow.key.is_some() => sqlx::query(&self.render(
                "SELECT id FROM workflows WHERE resource_key = ? \
                         AND ((org_id = ?) OR (org_id IS NULL AND ? IS NULL)) \
                         ORDER BY created_at, id LIMIT 1",
            ))
            .bind(workflow.artifact_key())
            .bind(workflow.org_id)
            .bind(workflow.org_id)
            .fetch_optional(self.pool())
            .await?
            .map(|row| row.get::<Uuid, _>("id")),
            None => {
                let sql = self.render(match &workflow.namespace {
                    Some(_) => "SELECT id FROM workflows WHERE name = ? AND namespace = ? ORDER BY created_at, id LIMIT 1",
                    None => "SELECT id FROM workflows WHERE name = ? AND namespace IS NULL ORDER BY created_at, id LIMIT 1",
                });
                let mut query = sqlx::query(&sql).bind(workflow.name.as_str());
                if workflow.namespace.is_some() {
                    query = query.bind(workflow.namespace.clone());
                }
                query
                    .fetch_optional(self.pool())
                    .await?
                    .map(|row| row.get::<Uuid, _>("id"))
            }
        };
        let workflow_id = existing_id.unwrap_or_else(Uuid::new_v4);
        let resource_key = workflow.artifact_key();

        // mysql has no usable RETURNING via sqlx: upsert with ON DUPLICATE KEY UPDATE, then read the
        // row back on the same pinned connection by the (now app-generated) id.
        if self.dialect() == SqlDialect::MariaDb {
            let conflict = SqlDialect::MariaDb.on_conflict_update(
                "id",
                &[
                    "name",
                    "resource_key",
                    "namespace",
                    "org_id",
                    "version",
                    "enabled",
                    "input_schema",
                    "definition",
                    "updated_at",
                ],
            );
            let mut conn = self.pool().acquire().await?;
            sqlx::query(&self.render(&format!(
                "INSERT INTO workflows ({WORKFLOW_COLUMNS})
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) {conflict}",
            )))
            .bind(workflow_id)
            .bind(workflow.name.as_str())
            .bind(resource_key)
            .bind(workflow.namespace.clone())
            .bind(workflow.org_id)
            .bind(workflow.version.to_string())
            .bind(workflow.enabled)
            .bind(serde_json::to_string(&workflow.input_type)?)
            .bind(workflow.definition.to_string())
            .bind(now)
            .bind(now)
            .execute(&mut *conn)
            .await?;
            let row = sqlx::query(&self.render(&format!(
                "SELECT {WORKFLOW_COLUMNS} FROM workflows WHERE id = ?"
            )))
            .bind(workflow_id)
            .fetch_one(&mut *conn)
            .await?;
            return Ok(mappers::row_to_workflow(&row));
        }

        let row = sqlx::query(&self.render(
            "INSERT INTO workflows (id, name, resource_key, namespace, org_id, version, enabled, input_schema, definition, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, resource_key = excluded.resource_key, namespace = excluded.namespace, org_id = excluded.org_id, version = excluded.version, enabled = excluded.enabled, input_schema = excluded.input_schema, definition = excluded.definition, updated_at = excluded.updated_at
             RETURNING id, name, resource_key, namespace, org_id, version, enabled, input_schema, definition, created_at, updated_at",
        ))
        .bind(workflow_id)
        .bind(workflow.name.as_str())
        .bind(resource_key)
        .bind(workflow.namespace.clone())
        .bind(workflow.org_id)
        .bind(workflow.version.to_string())
        .bind(workflow.enabled)
        .bind(serde_json::to_string(&workflow.input_type)?)
        .bind(workflow.definition.to_string())
        .bind(now)
        .bind(now)
        .fetch_one(self.pool())
        .await?;
        Ok(mappers::row_to_workflow(&row))
    }

    async fn insert_workflow(
        &self,
        workflow: &WorkflowDefinition,
    ) -> Result<WorkflowDefinition, SendableError> {
        // always insert a brand-new row: unlike upsert_workflow this never resolves an existing id
        // by name, so duplicating a workflow yields a sibling version sharing the same name.
        let now = Utc::now().timestamp();
        let id = Uuid::now_v7();
        let resource_key = format!("{}-{id}", workflow.artifact_key());

        if self.dialect() == SqlDialect::MariaDb {
            let mut conn = self.pool().acquire().await?;
            sqlx::query(&self.render(
                "INSERT INTO workflows ({WORKFLOW_COLUMNS})
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            ))
            .bind(id)
            .bind(workflow.name.as_str())
            .bind(&resource_key)
            .bind(workflow.namespace.clone())
            .bind(workflow.org_id)
            .bind(workflow.version.to_string())
            .bind(workflow.enabled)
            .bind(serde_json::to_string(&workflow.input_type)?)
            .bind(workflow.definition.to_string())
            .bind(now)
            .bind(now)
            .execute(&mut *conn)
            .await?;
            let row = sqlx::query(&self.render(&format!(
                "SELECT {WORKFLOW_COLUMNS} FROM workflows WHERE id = ?"
            )))
            .bind(id)
            .fetch_one(&mut *conn)
            .await?;
            return Ok(mappers::row_to_workflow(&row));
        }

        let row = sqlx::query(&self.render(&format!(
            "INSERT INTO workflows ({WORKFLOW_COLUMNS})
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             RETURNING {WORKFLOW_COLUMNS}",
        )))
        .bind(id)
        .bind(workflow.name.as_str())
        .bind(&resource_key)
        .bind(workflow.namespace.clone())
        .bind(workflow.org_id)
        .bind(workflow.version.to_string())
        .bind(workflow.enabled)
        .bind(serde_json::to_string(&workflow.input_type)?)
        .bind(workflow.definition.to_string())
        .bind(now)
        .bind(now)
        .fetch_one(self.pool())
        .await?;
        Ok(mappers::row_to_workflow(&row))
    }

    async fn fetch_workflows(&self) -> Result<Vec<WorkflowDefinition>, SendableError> {
        let rows = sqlx::query(&format!(
            "SELECT {WORKFLOW_COLUMNS} FROM workflows ORDER BY name"
        ))
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_workflow).collect())
    }

    async fn fetch_workflow_ids_for_org(&self, org_id: Uuid) -> Result<Vec<Uuid>, SendableError> {
        let rows = sqlx::query(&self.render("SELECT id FROM workflows WHERE org_id = ?"))
            .bind(org_id)
            .fetch_all(self.pool())
            .await?;
        let mut ids = Vec::with_capacity(rows.len());
        for row in &rows {
            ids.push(row.try_get("id")?);
        }
        Ok(ids)
    }

    async fn set_workflow_org(
        &self,
        workflow_id: Uuid,
        org_id: Option<Uuid>,
    ) -> Result<(), SendableError> {
        sqlx::query(&self.render("UPDATE workflows SET org_id = ? WHERE id = ?"))
            .bind(org_id)
            .bind(workflow_id)
            .execute(self.pool())
            .await?;
        Ok(())
    }

    async fn delete_workflow(&self, workflow_id: Uuid) -> Result<(), SendableError> {
        // Cascade-delete the workflow's VM execution history before the workflow row, since
        // workflow_runs.workflow_id is a restrict foreign key.  SQLite can run with foreign-key
        // enforcement disabled, so the explicit child-to-parent order is still required there.
        let run_filter = "workflow_run_id IN (SELECT id FROM workflow_runs WHERE workflow_id = ?)";
        let effect_filter = "continuation_id IN (SELECT id FROM workflow_continuations WHERE workflow_run_id IN (SELECT id FROM workflow_runs WHERE workflow_id = ?))";

        retry_delete(|| async {
            let mut tx = self.pool().begin().await?;
            for sql in [
                format!("DELETE FROM workflow_effect_output_events WHERE effect_id IN (SELECT id FROM workflow_effects WHERE {effect_filter})"),
                format!("DELETE FROM workflow_effect_dispatches WHERE effect_id IN (SELECT id FROM workflow_effects WHERE {effect_filter})"),
                format!("DELETE FROM workflow_journal_entries WHERE {run_filter}"),
                format!("DELETE FROM workflow_effects WHERE {effect_filter}"),
                format!("DELETE FROM workflow_continuations WHERE {run_filter}"),
                format!("DELETE FROM workflow_vm_modules WHERE {run_filter}"),
                format!("DELETE FROM workflow_trigger_firings WHERE {run_filter}"),
                "DELETE FROM workflow_runs WHERE workflow_id = ?".to_string(),
                // deleted explicitly rather than left to the declared cascade: sqlite only enforces
                // foreign keys when the pragma is on, so the history would otherwise outlive its workflow.
                "DELETE FROM workflow_revisions WHERE workflow_id = ?".to_string(),
                "DELETE FROM workflows WHERE id = ?".to_string(),
            ] {
                sqlx::query(&self.render(&sql))
                    .bind(workflow_id)
                    .execute(&mut *tx)
                    .await?;
            }
            tx.commit().await
        })
        .await?;
        Ok(())
    }

    async fn insert_workflow_revision(
        &self,
        revision: &WorkflowRevision,
    ) -> Result<Option<WorkflowRevision>, SendableError> {
        // serialized exactly as `workflows.definition` is, so the two columns stay comparable.
        let definition = revision.definition.to_string();
        let input_schema = serde_json::to_string(&revision.input_type)?;
        // The repository normally supplies this digest, but DefinitionStore is public and direct
        // callers must not be able to insert an unpinnable new revision by omitting it.
        let digest = if revision.digest.trim().is_empty() {
            WorkflowRevision::content_digest(
                revision.version,
                &revision.input_type,
                &revision.definition,
            )
        } else {
            revision.digest.clone()
        };

        // the unique (workflow_id, revision) index is what actually prevents a forked sequence, so a
        // concurrent writer that wins the number makes this insert fail rather than duplicate it.
        // recompute once against the new head instead of surfacing a conflict the caller cannot act on.
        for attempt in 0..2 {
            let head = sqlx::query(&self.render(
                "SELECT revision, digest, name, version, definition, input_schema FROM workflow_revisions \
                 WHERE workflow_id = ? ORDER BY revision DESC LIMIT 1",
            ))
            .bind(revision.workflow_id)
            .fetch_optional(self.pool())
            .await?;

            let next = match head {
                Some(row) => {
                    // an unchanged re-save records nothing: a pack that reapplies hourly would
                    // otherwise bury the edits worth seeing under identical rows.
                    let unchanged = row.get::<String, _>("definition") == definition
                        && row.get::<String, _>("input_schema") == input_schema
                        && row.get::<String, _>("name") == revision.name
                        && row.get::<String, _>("version") == revision.version.to_string();
                    if unchanged {
                        return Ok(None);
                    }
                    row.get::<i64, _>("revision") + 1
                }
                None => 1,
            };

            let id = Uuid::now_v7();
            let created_at = Utc::now().timestamp();
            let result = sqlx::query(&self.render(
                "INSERT INTO workflow_revisions \
                 (id, workflow_id, revision, digest, version, name, definition, input_schema, source, actor_id, actor_kind, note, created_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            ))
            .bind(id)
            .bind(revision.workflow_id)
            .bind(next)
            .bind(digest.as_str())
            .bind(revision.version.to_string())
            .bind(revision.name.as_str())
            .bind(definition.as_str())
            .bind(input_schema.as_str())
            .bind(revision.source.as_str())
            .bind(revision.actor_id)
            .bind(revision.actor_kind.as_str())
            .bind(revision.note.clone())
            .bind(created_at)
            .execute(self.pool())
            .await;

            match result {
                Ok(_) => {
                    return Ok(Some(WorkflowRevision {
                        id,
                        revision: next,
                        digest,
                        created_at: DateTime::<Utc>::from_timestamp(created_at, 0),
                        ..revision.clone()
                    }));
                }
                // only the sequence collision is retryable; anything else is a real failure.
                Err(err) if attempt == 0 && is_unique_violation(&err) => continue,
                Err(err) => return Err(Box::new(err)),
            }
        }

        Ok(None)
    }

    async fn fetch_workflow_revisions(
        &self,
        workflow_id: Uuid,
        limit: i64,
    ) -> Result<Vec<WorkflowRevision>, SendableError> {
        let rows = sqlx::query(&self.render(
            "SELECT id, workflow_id, revision, digest, version, name, definition, input_schema, source, \
             actor_id, actor_kind, note, created_at FROM workflow_revisions \
             WHERE workflow_id = ? ORDER BY revision DESC LIMIT ?",
        ))
        .bind(workflow_id)
        .bind(limit.max(1))
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_workflow_revision).collect())
    }

    async fn fetch_workflow_revision(
        &self,
        workflow_id: Uuid,
        revision: i64,
    ) -> Result<Option<WorkflowRevision>, SendableError> {
        let row = sqlx::query(&self.render(
            "SELECT id, workflow_id, revision, digest, version, name, definition, input_schema, source, \
             actor_id, actor_kind, note, created_at FROM workflow_revisions \
             WHERE workflow_id = ? AND revision = ?",
        ))
        .bind(workflow_id)
        .bind(revision)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.as_ref().map(mappers::row_to_workflow_revision))
    }

    async fn upsert_pipeline(&self, pipeline: &Pipeline) -> Result<Pipeline, SendableError> {
        let now = Utc::now().timestamp();
        let existing_id = match pipeline.id {
            Some(id) => Some(id),
            None if pipeline.key.is_some() => sqlx::query(&self.render(
                "SELECT id FROM pipelines WHERE resource_key = ? \
                     AND ((org_id = ?) OR (org_id IS NULL AND ? IS NULL)) \
                     ORDER BY created_at, id LIMIT 1",
            ))
            .bind(pipeline.artifact_key())
            .bind(pipeline.org_id)
            .bind(pipeline.org_id)
            .fetch_optional(self.pool())
            .await?
            .map(|row| row.get::<Uuid, _>("id")),
            None => None,
        };
        let pipeline_id = existing_id.unwrap_or_else(Uuid::new_v4);
        let resource_key = pipeline.artifact_key();
        let graph = serde_json::to_string(&pipeline.graph).unwrap_or_else(|_| "{}".to_string());
        let concurrency =
            serde_json::to_string(&pipeline.concurrency).unwrap_or_else(|_| "{}".to_string());
        let defaults =
            serde_json::to_string(&pipeline.defaults).unwrap_or_else(|_| "{}".to_string());

        let update_cols = [
            "name",
            "resource_key",
            "namespace",
            "description",
            "org_id",
            "defaults",
            "metadata",
            "graph",
            "concurrency",
            "updated_at",
        ];

        // mysql has no usable RETURNING via sqlx: upsert with ON DUPLICATE KEY UPDATE, then read the
        // row back on the same pinned connection by the (now app-generated) id.
        if self.dialect() == SqlDialect::MariaDb {
            let conflict = SqlDialect::MariaDb.on_conflict_update("id", &update_cols);
            let mut conn = self.pool().acquire().await?;
            sqlx::query(&self.render(&format!(
                "INSERT INTO pipelines ({PIPELINE_COLUMNS})
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) {conflict}",
            )))
            .bind(pipeline_id)
            .bind(&pipeline.name)
            .bind(resource_key)
            .bind(&pipeline.namespace)
            .bind(&pipeline.description)
            .bind(pipeline.org_id)
            .bind(&defaults)
            .bind(pipeline.metadata.to_string())
            .bind(&graph)
            .bind(&concurrency)
            .bind(pipeline.created_at.map(|dt| dt.timestamp()).unwrap_or(now))
            .bind(now)
            .execute(&mut *conn)
            .await?;
            let row = sqlx::query(&self.render(&format!(
                "SELECT {PIPELINE_COLUMNS} FROM pipelines WHERE id = ?"
            )))
            .bind(pipeline_id)
            .fetch_one(&mut *conn)
            .await?;
            return Ok(mappers::row_to_pipeline(&row));
        }

        let conflict = self.dialect().on_conflict_update("id", &update_cols);
        let row = sqlx::query(&self.render(&format!(
            "INSERT INTO pipelines ({PIPELINE_COLUMNS})
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) {conflict}
             RETURNING {PIPELINE_COLUMNS}",
        )))
        .bind(pipeline_id)
        .bind(&pipeline.name)
        .bind(resource_key)
        .bind(&pipeline.namespace)
        .bind(&pipeline.description)
        .bind(pipeline.org_id)
        .bind(&defaults)
        .bind(pipeline.metadata.to_string())
        .bind(&graph)
        .bind(&concurrency)
        .bind(pipeline.created_at.map(|dt| dt.timestamp()).unwrap_or(now))
        .bind(now)
        .fetch_one(self.pool())
        .await?;
        Ok(mappers::row_to_pipeline(&row))
    }

    async fn fetch_pipelines(&self) -> Result<Vec<Pipeline>, SendableError> {
        let rows = sqlx::query(&self.render(&format!(
            "SELECT {PIPELINE_COLUMNS} FROM pipelines ORDER BY name, id"
        )))
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_pipeline).collect())
    }

    async fn insert_pipeline_revision(
        &self,
        revision: &PipelineRevision,
    ) -> Result<Option<PipelineRevision>, SendableError> {
        let graph = serde_json::to_string(&revision.graph)?;
        let concurrency = serde_json::to_string(&revision.concurrency)?;
        let defaults = serde_json::to_string(&revision.defaults)?;
        let metadata = revision.metadata.to_string();
        let digest = if revision.digest.trim().is_empty() {
            PipelineRevision::content_digest(
                &revision.graph,
                &revision.concurrency,
                &revision.defaults,
                &revision.metadata,
            )
        } else {
            revision.digest.clone()
        };

        for attempt in 0..2 {
            let head = sqlx::query(&self.render(
                "SELECT revision, name, description, graph, concurrency, defaults, metadata \
                 FROM pipeline_revisions WHERE pipeline_id = ? ORDER BY revision DESC LIMIT 1",
            ))
            .bind(revision.pipeline_id)
            .fetch_optional(self.pool())
            .await?;
            let next = match head {
                Some(row) => {
                    let unchanged = row.get::<String, _>("name") == revision.name
                        && row.get::<Option<String>, _>("description") == revision.description
                        && row.get::<String, _>("graph") == graph
                        && row.get::<String, _>("concurrency") == concurrency
                        && row.get::<String, _>("defaults") == defaults
                        && row.get::<String, _>("metadata") == metadata;
                    if unchanged {
                        return Ok(None);
                    }
                    row.get::<i64, _>("revision") + 1
                }
                None => 1,
            };
            let id = Uuid::now_v7();
            let created_at = Utc::now().timestamp();
            let result = sqlx::query(&self.render(
                "INSERT INTO pipeline_revisions \
                 (id, pipeline_id, revision, digest, name, description, graph, concurrency, defaults, metadata, source, actor_id, actor_kind, note, created_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            ))
            .bind(id)
            .bind(revision.pipeline_id)
            .bind(next)
            .bind(&digest)
            .bind(&revision.name)
            .bind(&revision.description)
            .bind(&graph)
            .bind(&concurrency)
            .bind(&defaults)
            .bind(&metadata)
            .bind(revision.source.as_str())
            .bind(revision.actor_id)
            .bind(&revision.actor_kind)
            .bind(&revision.note)
            .bind(created_at)
            .execute(self.pool())
            .await;
            match result {
                Ok(_) => {
                    return Ok(Some(PipelineRevision {
                        id,
                        revision: next,
                        digest,
                        created_at: DateTime::<Utc>::from_timestamp(created_at, 0),
                        ..revision.clone()
                    }));
                }
                Err(err) if attempt == 0 && is_unique_violation(&err) => continue,
                Err(err) => return Err(Box::new(err)),
            }
        }
        Ok(None)
    }

    async fn fetch_pipeline_revisions(
        &self,
        pipeline_id: Uuid,
        limit: i64,
    ) -> Result<Vec<PipelineRevision>, SendableError> {
        let rows = sqlx::query(&self.render(&format!(
            "SELECT {PIPELINE_REVISION_COLUMNS} FROM pipeline_revisions \
             WHERE pipeline_id = ? ORDER BY revision DESC LIMIT ?"
        )))
        .bind(pipeline_id)
        .bind(limit.max(1))
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_pipeline_revision).collect())
    }

    async fn fetch_pipeline_revision(
        &self,
        pipeline_id: Uuid,
        revision: i64,
    ) -> Result<Option<PipelineRevision>, SendableError> {
        let row = sqlx::query(&self.render(&format!(
            "SELECT {PIPELINE_REVISION_COLUMNS} FROM pipeline_revisions \
             WHERE pipeline_id = ? AND revision = ?"
        )))
        .bind(pipeline_id)
        .bind(revision)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.as_ref().map(mappers::row_to_pipeline_revision))
    }

    async fn delete_pipeline(&self, pipeline_id: Uuid) -> Result<(), SendableError> {
        retry_delete(|| async {
            let mut tx = self.pool().begin().await?;
            let pipeline_runs = "pipeline_run_id IN (SELECT id FROM pipeline_runs WHERE pipeline_id = ?)";
            let workflow_runs = "workflow_run_id IN (SELECT id FROM workflow_runs WHERE pipeline_run_id IN (SELECT id FROM pipeline_runs WHERE pipeline_id = ?))";
            let workflow_effects = "continuation_id IN (SELECT id FROM workflow_continuations WHERE workflow_run_id IN (SELECT id FROM workflow_runs WHERE pipeline_run_id IN (SELECT id FROM pipeline_runs WHERE pipeline_id = ?)))";

            // Pipeline and member workflow runs use restrict-mode foreign keys, so remove their
            // execution history child-to-parent before deleting the definitions.
            for sql in [
                format!("DELETE FROM pipeline_trigger_firings WHERE {pipeline_runs}"),
                format!("DELETE FROM pipeline_member_attempts WHERE {pipeline_runs}"),
                format!("DELETE FROM workflow_effect_output_events WHERE effect_id IN (SELECT id FROM workflow_effects WHERE {workflow_effects})"),
                format!("DELETE FROM workflow_effect_dispatches WHERE effect_id IN (SELECT id FROM workflow_effects WHERE {workflow_effects})"),
                format!("DELETE FROM workflow_journal_entries WHERE {workflow_runs}"),
                format!("DELETE FROM workflow_effects WHERE {workflow_effects}"),
                format!("DELETE FROM workflow_continuations WHERE {workflow_runs}"),
                format!("DELETE FROM workflow_vm_modules WHERE {workflow_runs}"),
                format!("DELETE FROM workflow_trigger_firings WHERE {workflow_runs}"),
                "DELETE FROM workflow_runs WHERE pipeline_run_id IN (SELECT id FROM pipeline_runs WHERE pipeline_id = ?)".to_string(),
                "DELETE FROM pipeline_runs WHERE pipeline_id = ?".to_string(),
                "DELETE FROM pipeline_revisions WHERE pipeline_id = ?".to_string(),
                "DELETE FROM pipelines WHERE id = ?".to_string(),
            ] {
                sqlx::query(&self.render(&sql))
                    .bind(pipeline_id)
                    .execute(&mut *tx)
                    .await?;
            }
            tx.commit().await
        })
        .await?;
        Ok(())
    }

    async fn fetch_pipeline_ids_for_org(&self, org_id: Uuid) -> Result<Vec<Uuid>, SendableError> {
        let rows = sqlx::query(&self.render("SELECT id FROM pipelines WHERE org_id = ?"))
            .bind(org_id)
            .fetch_all(self.pool())
            .await?;
        let mut ids = Vec::with_capacity(rows.len());
        for row in &rows {
            ids.push(row.try_get("id")?);
        }
        Ok(ids)
    }

    async fn set_pipeline_org(
        &self,
        pipeline_id: Uuid,
        org_id: Option<Uuid>,
    ) -> Result<(), SendableError> {
        sqlx::query(&self.render("UPDATE pipelines SET org_id = ?, updated_at = ? WHERE id = ?"))
            .bind(org_id)
            .bind(Utc::now().timestamp())
            .bind(pipeline_id)
            .execute(self.pool())
            .await?;
        Ok(())
    }

    async fn fetch_recent_pipeline_runs(
        &self,
        limit: i64,
    ) -> Result<Vec<PipelineRun>, SendableError> {
        let rows = sqlx::query(&self.render(&format!(
            "SELECT {PIPELINE_RUN_COLUMNS} FROM pipeline_runs ORDER BY created_at DESC, id DESC LIMIT ?"
        )))
        .bind(limit.max(1))
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_pipeline_run).collect())
    }

    async fn fetch_pipeline_runs_for_pipeline(
        &self,
        pipeline_id: Uuid,
    ) -> Result<Vec<PipelineRun>, SendableError> {
        let rows = sqlx::query(&self.render(&format!(
            "SELECT {PIPELINE_RUN_COLUMNS} FROM pipeline_runs WHERE pipeline_id = ? ORDER BY created_at DESC, id DESC"
        )))
        .bind(pipeline_id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_pipeline_run).collect())
    }

    async fn delete_pipeline_run(&self, pipeline_run_id: Uuid) -> Result<(), SendableError> {
        retry_delete(|| async {
            let mut tx = self.pool().begin().await?;
            for sql in [
                "DELETE FROM pipeline_trigger_firings WHERE pipeline_run_id = ?",
                "DELETE FROM pipeline_member_attempts WHERE pipeline_run_id = ?",
                "DELETE FROM workflow_effect_output_events WHERE effect_id IN (SELECT id FROM workflow_effects WHERE continuation_id IN (SELECT id FROM workflow_continuations WHERE workflow_run_id IN (SELECT id FROM workflow_runs WHERE pipeline_run_id = ?)))",
                "DELETE FROM workflow_effect_dispatches WHERE effect_id IN (SELECT id FROM workflow_effects WHERE continuation_id IN (SELECT id FROM workflow_continuations WHERE workflow_run_id IN (SELECT id FROM workflow_runs WHERE pipeline_run_id = ?)))",
                "DELETE FROM workflow_journal_entries WHERE workflow_run_id IN (SELECT id FROM workflow_runs WHERE pipeline_run_id = ?)",
                "DELETE FROM workflow_effects WHERE continuation_id IN (SELECT id FROM workflow_continuations WHERE workflow_run_id IN (SELECT id FROM workflow_runs WHERE pipeline_run_id = ?))",
                "DELETE FROM workflow_continuations WHERE workflow_run_id IN (SELECT id FROM workflow_runs WHERE pipeline_run_id = ?)",
                "DELETE FROM workflow_vm_modules WHERE workflow_run_id IN (SELECT id FROM workflow_runs WHERE pipeline_run_id = ?)",
                "DELETE FROM workflow_runs WHERE pipeline_run_id = ?",
                "DELETE FROM pipeline_runs WHERE id = ?",
            ] {
                sqlx::query(&self.render(sql)).bind(pipeline_run_id).execute(&mut *tx).await?;
            }
            tx.commit().await
        }).await?;
        Ok(())
    }

    async fn upsert_catalog_item(&self, item: Value) -> Result<Value, SendableError> {
        let now = Utc::now().timestamp();
        // catalog_items.id is a UUID primary key with no database default. Generate it on insert.
        // On a URI conflict, the update does not touch id, so existing rows keep theirs.
        let id = Uuid::new_v4();
        let columns =
            "id, uri, item_type, name, version, document, metadata, created_at, updated_at";
        let document = item
            .get("document")
            .cloned()
            .unwrap_or(Value::Object(Default::default()))
            .to_string();

        if self.dialect() == SqlDialect::MariaDb {
            let conflict = SqlDialect::MariaDb.on_conflict_update(
                "uri",
                &[
                    "item_type",
                    "name",
                    "version",
                    "document",
                    "metadata",
                    "updated_at",
                ],
            );
            sqlx::query(&self.render(&format!(
                "INSERT INTO catalog_items (id, uri, item_type, name, version, document, metadata, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) {conflict}",
            )))
            .bind(id)
            .bind(json_str(&item, "uri"))
            .bind(json_str(&item, "item_type"))
            .bind(json_str(&item, "name"))
            .bind(json_str(&item, "version"))
            .bind(document)
            .bind(json_metadata(&item))
            .bind(now)
            .bind(now)
            .execute(self.pool())
            .await?;
            let row = sqlx::query(&self.render(&format!(
                "SELECT {columns} FROM catalog_items WHERE uri = ?",
            )))
            .bind(json_str(&item, "uri"))
            .fetch_one(self.pool())
            .await?;
            return Ok(mappers::row_to_catalog_item(&row));
        }

        let row = sqlx::query(&self.render(
            "INSERT INTO catalog_items (id, uri, item_type, name, version, document, metadata, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(uri) DO UPDATE SET item_type = excluded.item_type, name = excluded.name, version = excluded.version, document = excluded.document, metadata = excluded.metadata, updated_at = excluded.updated_at
             RETURNING id, uri, item_type, name, version, document, metadata, created_at, updated_at",
        ))
        .bind(id)
        .bind(json_str(&item, "uri"))
        .bind(json_str(&item, "item_type"))
        .bind(json_str(&item, "name"))
        .bind(json_str(&item, "version"))
        .bind(document)
        .bind(json_metadata(&item))
        .bind(now)
        .bind(now)
        .fetch_one(self.pool())
        .await?;
        Ok(mappers::row_to_catalog_item(&row))
    }

    async fn fetch_catalog_items(
        &self,
        item_type: Option<String>,
    ) -> Result<Vec<Value>, SendableError> {
        let rows = if let Some(item_type) = item_type {
            sqlx::query(&self.render("SELECT id, uri, item_type, name, version, document, metadata, created_at, updated_at FROM catalog_items WHERE item_type = ? ORDER BY uri"))
                .bind(item_type)
                .fetch_all(self.pool())
                .await?
        } else {
            sqlx::query("SELECT id, uri, item_type, name, version, document, metadata, created_at, updated_at FROM catalog_items ORDER BY uri")
                .fetch_all(self.pool())
                .await?
        };
        Ok(rows.iter().map(mappers::row_to_catalog_item).collect())
    }

    async fn fetch_catalog_item(&self, uri: String) -> Result<Option<Value>, SendableError> {
        let row = sqlx::query(&self.render("SELECT id, uri, item_type, name, version, document, metadata, created_at, updated_at FROM catalog_items WHERE uri = ?"))
            .bind(uri)
            .fetch_optional(self.pool())
            .await?;
        Ok(row.map(|row| mappers::row_to_catalog_item(&row)))
    }

    async fn delete_catalog_item(&self, uri: String) -> Result<bool, SendableError> {
        let result = sqlx::query(&self.render("DELETE FROM catalog_items WHERE uri = ?"))
            .bind(uri)
            .execute(self.pool())
            .await?;
        Ok(result.affected() > 0)
    }
}
