//! triggers, firings, and freeze windows.
//!
//! the `ScheduleStore` half of the generic sql implementation. bodies are written once, over any
//! `SqlBackend`; see `super` for the shared helpers they call.

use super::*;

// the bound list is repeated verbatim in every role impl in this directory. it stays spelled out
// rather than hidden behind a macro so that type errors inside the query bodies — the part that
// actually gets edited — keep pointing at real source lines instead of a macro expansion.
impl<B> ScheduleStore for SqlStore<B>
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
    async fn upsert_workflow_trigger(
        &self,
        trigger: &WorkflowTrigger,
    ) -> Result<WorkflowTrigger, SendableError> {
        let now = Utc::now().timestamp();
        let trigger_id = trigger.id.unwrap_or_else(Uuid::new_v4);

        // mysql has no usable RETURNING via sqlx: upsert with ON DUPLICATE KEY UPDATE, then read the
        // row back on the same pinned connection by the (now app-generated) id.
        if self.dialect() == SqlDialect::MariaDb {
            let columns = "id, workflow_id, kind, enabled, configuration, next_execution, blackout_start, blackout_end, metadata, created_at, updated_at";
            let conflict = SqlDialect::MariaDb.on_conflict_update(
                "id",
                &[
                    "workflow_id",
                    "kind",
                    "enabled",
                    "configuration",
                    "next_execution",
                    "blackout_start",
                    "blackout_end",
                    "metadata",
                    "updated_at",
                ],
            );
            let mut conn = self.pool().acquire().await?;
            sqlx::query(&self.render(&format!(
                "INSERT INTO workflow_triggers (id, workflow_id, kind, enabled, configuration, next_execution, blackout_start, blackout_end, metadata, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) {conflict}",
            )))
            .bind(trigger_id)
            .bind(trigger.workflow_id)
            .bind(trigger.kind.as_str())
            .bind(trigger.enabled)
            .bind(trigger.configuration.to_string())
            .bind(trigger.next_execution.map(|dt| dt.timestamp()))
            .bind(trigger.blackout_start.map(|dt| dt.timestamp()))
            .bind(trigger.blackout_end.map(|dt| dt.timestamp()))
            .bind(trigger.metadata.to_string())
            .bind(trigger.created_at.map(|dt| dt.timestamp()).unwrap_or(now))
            .bind(now)
            .execute(&mut *conn)
            .await?;
            let row = sqlx::query(&self.render(&format!(
                "SELECT {columns} FROM workflow_triggers WHERE id = ?"
            )))
            .bind(trigger_id)
            .fetch_one(&mut *conn)
            .await?;
            return Ok(mappers::row_to_workflow_trigger(&row));
        }

        let row = sqlx::query(&self.render(
            "INSERT INTO workflow_triggers (id, workflow_id, kind, enabled, configuration, next_execution, blackout_start, blackout_end, metadata, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET workflow_id = excluded.workflow_id, kind = excluded.kind, enabled = excluded.enabled, configuration = excluded.configuration, next_execution = excluded.next_execution, blackout_start = excluded.blackout_start, blackout_end = excluded.blackout_end, metadata = excluded.metadata, updated_at = excluded.updated_at
             RETURNING id, workflow_id, kind, enabled, configuration, next_execution, blackout_start, blackout_end, metadata, created_at, updated_at",
        ))
        .bind(trigger_id)
        .bind(trigger.workflow_id)
        .bind(trigger.kind.as_str())
        .bind(trigger.enabled)
        .bind(trigger.configuration.to_string())
        .bind(trigger.next_execution.map(|dt| dt.timestamp()))
        .bind(trigger.blackout_start.map(|dt| dt.timestamp()))
        .bind(trigger.blackout_end.map(|dt| dt.timestamp()))
        .bind(trigger.metadata.to_string())
        .bind(trigger.created_at.map(|dt| dt.timestamp()).unwrap_or(now))
        .bind(now)
        .fetch_one(self.pool())
        .await?;
        Ok(mappers::row_to_workflow_trigger(&row))
    }

    async fn fetch_workflow_trigger(
        &self,
        trigger_id: Uuid,
    ) -> Result<Option<WorkflowTrigger>, SendableError> {
        let row = sqlx::query(&self.render("SELECT id, workflow_id, kind, enabled, configuration, next_execution, blackout_start, blackout_end, metadata, created_at, updated_at FROM workflow_triggers WHERE id = ?"))
            .bind(trigger_id)
            .fetch_optional(self.pool())
            .await?;
        Ok(row.map(|row| mappers::row_to_workflow_trigger(&row)))
    }

    async fn delete_workflow_trigger(&self, trigger_id: Uuid) -> Result<(), SendableError> {
        retry_delete(|| async {
            self.pool()
                .execute(
                    sqlx::query(&self.render("DELETE FROM workflow_triggers WHERE id = ?"))
                        .bind(trigger_id),
                )
                .await
                .map(|_| ())
        })
        .await?;
        Ok(())
    }

    async fn upsert_pipeline_trigger(
        &self,
        trigger: &PipelineTrigger,
    ) -> Result<PipelineTrigger, SendableError> {
        let now = Utc::now().timestamp();
        let trigger_id = trigger.id.unwrap_or_else(Uuid::new_v4);
        let update_cols = [
            "pipeline_id",
            "kind",
            "enabled",
            "configuration",
            "next_execution",
            "blackout_start",
            "blackout_end",
            "metadata",
            "updated_at",
        ];

        // mysql has no usable RETURNING via sqlx: upsert, then read the row back on the same conn.
        if self.dialect() == SqlDialect::MariaDb {
            let conflict = SqlDialect::MariaDb.on_conflict_update("id", &update_cols);
            let mut conn = self.pool().acquire().await?;
            sqlx::query(&self.render(&format!(
                "INSERT INTO pipeline_triggers ({PIPELINE_TRIGGER_COLUMNS})
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) {conflict}",
            )))
            .bind(trigger_id)
            .bind(trigger.pipeline_id)
            .bind(trigger.kind.as_str())
            .bind(trigger.enabled)
            .bind(trigger.configuration.to_string())
            .bind(trigger.next_execution.map(|dt| dt.timestamp()))
            .bind(trigger.blackout_start.map(|dt| dt.timestamp()))
            .bind(trigger.blackout_end.map(|dt| dt.timestamp()))
            .bind(trigger.metadata.to_string())
            .bind(trigger.created_at.map(|dt| dt.timestamp()).unwrap_or(now))
            .bind(now)
            .execute(&mut *conn)
            .await?;
            let row = sqlx::query(&self.render(&format!(
                "SELECT {PIPELINE_TRIGGER_COLUMNS} FROM pipeline_triggers WHERE id = ?"
            )))
            .bind(trigger_id)
            .fetch_one(&mut *conn)
            .await?;
            return Ok(mappers::row_to_pipeline_trigger(&row));
        }

        let conflict = self.dialect().on_conflict_update("id", &update_cols);
        let row = sqlx::query(&self.render(&format!(
            "INSERT INTO pipeline_triggers ({PIPELINE_TRIGGER_COLUMNS})
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) {conflict}
             RETURNING {PIPELINE_TRIGGER_COLUMNS}",
        )))
        .bind(trigger_id)
        .bind(trigger.pipeline_id)
        .bind(trigger.kind.as_str())
        .bind(trigger.enabled)
        .bind(trigger.configuration.to_string())
        .bind(trigger.next_execution.map(|dt| dt.timestamp()))
        .bind(trigger.blackout_start.map(|dt| dt.timestamp()))
        .bind(trigger.blackout_end.map(|dt| dt.timestamp()))
        .bind(trigger.metadata.to_string())
        .bind(trigger.created_at.map(|dt| dt.timestamp()).unwrap_or(now))
        .bind(now)
        .fetch_one(self.pool())
        .await?;
        Ok(mappers::row_to_pipeline_trigger(&row))
    }

    async fn fetch_pipeline_triggers(
        &self,
        pipeline_id: Uuid,
    ) -> Result<Vec<PipelineTrigger>, SendableError> {
        let rows = sqlx::query(&self.render(&format!(
            "SELECT {PIPELINE_TRIGGER_COLUMNS} FROM pipeline_triggers WHERE pipeline_id = ? ORDER BY created_at, id"
        )))
        .bind(pipeline_id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_pipeline_trigger).collect())
    }

    async fn fetch_pipeline_trigger(
        &self,
        trigger_id: Uuid,
    ) -> Result<Option<PipelineTrigger>, SendableError> {
        let row = sqlx::query(&self.render(&format!(
            "SELECT {PIPELINE_TRIGGER_COLUMNS} FROM pipeline_triggers WHERE id = ?"
        )))
        .bind(trigger_id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|row| mappers::row_to_pipeline_trigger(&row)))
    }

    async fn delete_pipeline_trigger(&self, trigger_id: Uuid) -> Result<(), SendableError> {
        retry_delete(|| async {
            self.pool()
                .execute(
                    sqlx::query(&self.render("DELETE FROM pipeline_triggers WHERE id = ?"))
                        .bind(trigger_id),
                )
                .await
                .map(|_| ())
        })
        .await?;
        Ok(())
    }

    async fn claim_due_pipeline_trigger_firings(
        &self,
        scheduler_id: String,
        now: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<PipelineRun>, SendableError> {
        let mut tx = self.pool().begin().await?;
        let select_sql = self.render(&format!(
            "SELECT {PIPELINE_TRIGGER_COLUMNS} FROM pipeline_triggers \
             WHERE enabled = {} AND kind = 'cron' AND (next_execution IS NULL OR next_execution <= ?) \
               AND NOT EXISTS ({}) \
             ORDER BY COALESCE(next_execution, 0), id LIMIT ?{}",
            self.dialect().bool_true(),
            active_freeze_window_sql(self.dialect(), PIPELINE_FREEZE_SCOPE),
            self.dialect().skip_locked(),
        ));
        let rows = sqlx::query(&select_sql)
            .bind(now.timestamp())
            .bind(now.timestamp())
            .bind(now.timestamp())
            .bind(limit.max(1))
            .fetch_all(&mut *tx)
            .await?;

        let firing_sql = self.render(&self.dialect().insert_ignore(
            "pipeline_trigger_firings",
            "id, trigger_id, fire_key, scheduler_id, created_at",
            "?, ?, ?, ?, ?",
            "trigger_id, fire_key",
            None,
        ));
        let update_next_sql = self
            .render("UPDATE pipeline_triggers SET next_execution = ?, updated_at = ? WHERE id = ?");

        let mut runs = Vec::new();
        for row in rows {
            let mut trigger = mappers::row_to_pipeline_trigger(&row);
            let Some(trigger_id) = trigger.id else {
                continue;
            };
            let cron_schedule = trigger
                .configuration
                .get("cron")
                .and_then(Value::as_str)
                .unwrap_or_default();

            if trigger.next_execution.is_none() {
                trigger.next_execution = Some(next_execution_for_cron(cron_schedule, now)?);
                sqlx::query(&update_next_sql)
                    .bind(trigger.next_execution.map(|dt| dt.timestamp()))
                    .bind(now.timestamp())
                    .bind(trigger_id)
                    .execute(&mut *tx)
                    .await?;
                continue;
            }

            if trigger.is_pipeline_trigger_in_blackout(now) {
                if let Some(end) = trigger.blackout_end {
                    sqlx::query(&update_next_sql)
                        .bind(end.timestamp())
                        .bind(now.timestamp())
                        .bind(trigger_id)
                        .execute(&mut *tx)
                        .await?;
                }
                continue;
            }

            let fire_key = trigger
                .next_execution
                .map(|dt| dt.timestamp().to_string())
                .unwrap_or_else(|| "initial".into());
            let insert = sqlx::query(&firing_sql)
                .bind(Uuid::now_v7())
                .bind(trigger_id)
                .bind(fire_key.as_str())
                .bind(scheduler_id.as_str())
                .bind(now.timestamp())
                .execute(&mut *tx)
                .await?;
            if insert.affected() == 0 {
                continue;
            }

            let pipeline_row = sqlx::query(&self.render(&format!(
                "SELECT {PIPELINE_COLUMNS} FROM pipelines WHERE id = ?"
            )))
            .bind(trigger.pipeline_id)
            .fetch_one(&mut *tx)
            .await?;
            let pipeline_snapshot = mappers::row_to_pipeline(&pipeline_row);
            let new_run_id = Uuid::now_v7();
            let snapshot_json = serde_json::to_string(&pipeline_snapshot)?;
            let parameters = trigger.pipeline_trigger_parameters().to_string();
            let state = trigger.pipeline_trigger_state().to_string();
            let run_row = if self.dialect() == SqlDialect::MariaDb {
                sqlx::query(&self.render(
                    "INSERT INTO pipeline_runs (id, pipeline_id, pipeline_snapshot, status, parameters, state, created_at, trigger_source_kind, trigger_actor_type, trigger_actor_replica_id, trigger_actor_display_name, trigger_metadata) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, ?, ?)",
                ))
                .bind(new_run_id)
                .bind(trigger.pipeline_id)
                .bind(&snapshot_json)
                .bind(WorkflowStatus::Queued.as_str())
                .bind(&parameters)
                .bind(&state)
                .bind(now.timestamp())
                .bind("cron")
                .bind("replica")
                .bind(scheduler_id.as_str())
                .bind(trigger.metadata.to_string())
                .execute(&mut *tx)
                .await?;
                sqlx::query(&self.render(&format!(
                    "SELECT {PIPELINE_RUN_COLUMNS} FROM pipeline_runs WHERE id = ?"
                )))
                .bind(new_run_id)
                .fetch_one(&mut *tx)
                .await?
            } else {
                sqlx::query(&self.render(&format!(
                    "INSERT INTO pipeline_runs (id, pipeline_id, pipeline_snapshot, status, parameters, state, created_at, trigger_source_kind, trigger_actor_type, trigger_actor_replica_id, trigger_actor_display_name, trigger_metadata)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, ?, ?)
                     RETURNING {PIPELINE_RUN_COLUMNS}",
                )))
                .bind(new_run_id)
                .bind(trigger.pipeline_id)
                .bind(&snapshot_json)
                .bind(WorkflowStatus::Queued.as_str())
                .bind(&parameters)
                .bind(&state)
                .bind(now.timestamp())
                .bind("cron")
                .bind("replica")
                .bind(scheduler_id.as_str())
                .bind(trigger.metadata.to_string())
                .fetch_one(&mut *tx)
                .await?
            };
            let run = mappers::row_to_pipeline_run(&run_row);

            sqlx::query(&self.render("UPDATE pipeline_trigger_firings SET pipeline_run_id = ? WHERE trigger_id = ? AND fire_key = ?"))
                .bind(run.id)
                .bind(trigger_id)
                .bind(fire_key.as_str())
                .execute(&mut *tx)
                .await?;

            let next_execution = next_execution_for_cron(cron_schedule, now)?;
            sqlx::query(&update_next_sql)
                .bind(next_execution.timestamp())
                .bind(now.timestamp())
                .bind(trigger_id)
                .execute(&mut *tx)
                .await?;
            runs.push(run);
        }

        tx.commit().await?;
        Ok(runs)
    }

    async fn fetch_due_workflow_triggers(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Vec<WorkflowTrigger>, SendableError> {
        let sql = self.render(&format!(
            "SELECT id, workflow_id, kind, enabled, configuration, next_execution, blackout_start, blackout_end, metadata, created_at, updated_at FROM workflow_triggers WHERE enabled = {} AND kind = 'cron' AND EXISTS ({}) AND (next_execution IS NULL OR next_execution <= ?) ORDER BY COALESCE(next_execution, 0), id",
            self.dialect().bool_true(),
            workflow_enabled_sql(self.dialect()),
        ));
        let rows = sqlx::query(&sql)
            .bind(now.timestamp())
            .fetch_all(self.pool())
            .await?;
        Ok(rows.iter().map(mappers::row_to_workflow_trigger).collect())
    }

    async fn update_workflow_trigger_next_execution(
        &self,
        trigger_id: Uuid,
        next_execution: Option<DateTime<Utc>>,
    ) -> Result<(), SendableError> {
        self.pool()
            .execute(
                sqlx::query(&self.render(
                    "UPDATE workflow_triggers SET next_execution = ?, updated_at = ? WHERE id = ?",
                ))
                .bind(next_execution.map(|dt| dt.timestamp()))
                .bind(Utc::now().timestamp())
                .bind(trigger_id),
            )
            .await?;
        Ok(())
    }

    async fn claim_due_workflow_trigger_firings(
        &self,
        scheduler_id: String,
        now: DateTime<Utc>,
        limit: i64,
        modules: HashMap<Uuid, ScheduledWorkflowVm>,
    ) -> Result<TriggerFiringBatch<WorkflowRun>, SendableError> {
        let mut tx = self.pool().begin().await?;
        // frozen triggers, and triggers on a disabled workflow, are excluded in sql rather than
        // skipped in the loop below. either one leaves the slot due, so such a trigger would
        // otherwise sit at the head of the due ordering for the whole window and crowd every other
        // trigger out of the claim limit.
        let select_sql = self.render(&format!(
            "SELECT id, workflow_id, kind, enabled, configuration, next_execution, blackout_start, blackout_end, metadata, created_at, updated_at \
             FROM workflow_triggers \
             WHERE enabled = {} AND kind = 'cron' AND (next_execution IS NULL OR next_execution <= ?) \
               AND EXISTS ({}) \
               AND NOT EXISTS ({}) \
             ORDER BY COALESCE(next_execution, 0), id LIMIT ?{}",
            self.dialect().bool_true(),
            workflow_enabled_sql(self.dialect()),
            active_freeze_window_sql(self.dialect(), WORKFLOW_FREEZE_SCOPE),
            self.dialect().skip_locked(),
        ));
        let rows = sqlx::query(&select_sql)
            .bind(now.timestamp())
            .bind(now.timestamp())
            .bind(now.timestamp())
            .bind(limit.max(1))
            .fetch_all(&mut *tx)
            .await?;

        let update_next_sql = self
            .render("UPDATE workflow_triggers SET next_execution = ?, updated_at = ? WHERE id = ?");

        let mut batch = TriggerFiringBatch::default();
        for row in rows {
            let mut trigger = mappers::row_to_workflow_trigger(&row);
            let Some(trigger_id) = trigger.id else {
                continue;
            };
            let cron_schedule = trigger
                .configuration
                .get("cron")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();

            // first sighting: anchor the schedule without firing, so a freshly created trigger does
            // not read "never ran" as a missed slot.
            if trigger.next_execution.is_none() {
                trigger.next_execution = Some(next_execution_for_cron(&cron_schedule, now)?);
                sqlx::query(&update_next_sql)
                    .bind(trigger.next_execution.map(|dt| dt.timestamp()))
                    .bind(now.timestamp())
                    .bind(trigger_id)
                    .execute(&mut *tx)
                    .await?;
                continue;
            }

            if trigger.is_trigger_in_blackout(now) {
                if let Some(end) = trigger.blackout_end {
                    sqlx::query(&update_next_sql)
                        .bind(end.timestamp())
                        .bind(now.timestamp())
                        .bind(trigger_id)
                        .execute(&mut *tx)
                        .await?;
                }
                continue;
            }

            let Some(due) = trigger.next_execution else {
                continue;
            };
            let catchup = TriggerCatchup::from_configuration(&trigger.configuration);

            // a `skip` catch-up abandons slots that came due while nothing was firing them. the
            // grace matters: every firing is a little late, so without it `skip` would drop them all.
            if catchup.policy == CatchupPolicy::Skip
                && now.timestamp() - due.timestamp() > catchup.grace()
            {
                if self
                    .claim_firing_slot(
                        &mut tx,
                        trigger_id,
                        &due.timestamp().to_string(),
                        &scheduler_id,
                        FiringOutcome::CatchupSkipped,
                        now,
                    )
                    .await?
                {
                    batch.catchup_skipped += 1;
                }
                sqlx::query(&update_next_sql)
                    .bind(next_execution_for_cron(&cron_schedule, now)?.timestamp())
                    .bind(now.timestamp())
                    .bind(trigger_id)
                    .execute(&mut *tx)
                    .await?;
                continue;
            }

            // `fire_all` replays each missed slot as its own run, capped per pass; the re-anchor
            // then lands on the first slot it did not reach, so any remainder drains next tick.
            // every other policy collapses the backlog into the one due slot.
            let (slots, next_execution) = match catchup.policy {
                CatchupPolicy::FireAll => {
                    let (mut slots, _) = cron_slots_between(
                        &cron_schedule,
                        due,
                        now,
                        catchup.max_slots().saturating_sub(1),
                    )?;
                    slots.insert(0, due);
                    let last = slots.last().copied().unwrap_or(due);
                    let next = next_execution_for_cron(&cron_schedule, last)?;
                    (slots, next)
                }
                _ => (vec![due], next_execution_for_cron(&cron_schedule, now)?),
            };

            let Some(workflow_vm) = modules.get(&trigger.workflow_id) else {
                // The repository snapshots modules before entering this transaction. A trigger
                // that became due between that read and this claim remains due for the next pass.
                continue;
            };
            if workflow_vm.snapshot.id != Some(trigger.workflow_id) {
                return Err(crate::errors::WORKFLOW_VM_CORRUPT_STATE
                    .error("scheduled module snapshot names a different workflow"));
            }
            let workflow_snapshot = &workflow_vm.snapshot;
            let module = &workflow_vm.module;
            // the concurrency limit rides in the definition, so it versions with the workflow the
            // same way its triggers and alert policies do. the count is taken once per trigger and
            // then tracked locally as this pass creates runs.
            let concurrency =
                WorkflowConcurrency::from_metadata(&workflow_snapshot.definition.metadata);
            let mut active = match concurrency.is_enforced() {
                true => self.active_run_count(&mut tx, trigger.workflow_id).await?,
                false => 0,
            };

            let mut deferred = false;
            for slot in slots {
                if concurrency.is_enforced() && active >= concurrency.max_concurrent_runs {
                    match concurrency.on_conflict {
                        ConcurrencyPolicy::Skip => {
                            if self
                                .claim_firing_slot(
                                    &mut tx,
                                    trigger_id,
                                    &slot.timestamp().to_string(),
                                    &scheduler_id,
                                    FiringOutcome::ConcurrencySkipped,
                                    now,
                                )
                                .await?
                            {
                                batch.concurrency_skipped += 1;
                            }
                            continue;
                        }
                        ConcurrencyPolicy::Queue => {
                            batch.concurrency_deferred += 1;
                            deferred = true;
                            break;
                        }
                        ConcurrencyPolicy::CancelPrevious => {
                            let canceled = self
                                .cancel_active_runs(&mut tx, trigger.workflow_id, now)
                                .await?;
                            active = 0;
                            batch.canceled_run_ids.extend(canceled);
                        }
                        ConcurrencyPolicy::Allow => {}
                    }
                }

                // the firing row is the real gate on double-firing a slot: two replicas may both
                // pass the concurrency check, but only one insert wins.
                if !self
                    .claim_firing_slot(
                        &mut tx,
                        trigger_id,
                        &slot.timestamp().to_string(),
                        &scheduler_id,
                        FiringOutcome::Fired,
                        now,
                    )
                    .await?
                {
                    continue;
                }
                let run = self
                    .insert_trigger_run(
                        &mut tx,
                        &trigger,
                        workflow_snapshot,
                        &scheduler_id,
                        slot,
                        now,
                        module,
                    )
                    .await?;
                active += 1;
                batch.runs.push(run);
            }

            // a `queue` deferral leaves next_execution alone on purpose: the slot stays due and
            // fires as soon as capacity frees up, so the schedule itself is the queue and no run is
            // created to sit parked.
            if deferred {
                continue;
            }
            // a still-past `next_execution` here is deliberate and only happens under `fire_all`:
            // it is the first slot the per-pass cap did not reach, so the next tick keeps draining.
            // every other policy anchored from `now`, so its value is already in the future.
            sqlx::query(&update_next_sql)
                .bind(next_execution.timestamp())
                .bind(now.timestamp())
                .bind(trigger_id)
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await?;
        Ok(batch)
    }

    async fn backfill_workflow_trigger(
        &self,
        trigger_id: Uuid,
        request: &BackfillRequest,
        workflow_vm: ScheduledWorkflowVm,
    ) -> Result<(BackfillResponse, Vec<WorkflowRun>), SendableError> {
        let Some(trigger) = self.fetch_workflow_trigger(trigger_id).await? else {
            return Err(crate::errors::TRIGGER_NOT_FOUND.error(trigger_id));
        };
        let cron_schedule = trigger
            .configuration
            .get("cron")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if cron_schedule.is_empty() {
            return Err(crate::errors::TRIGGER_NOT_CRON.error(trigger_id));
        }

        let limit = request
            .limit
            .unwrap_or(DEFAULT_BACKFILL_LIMIT)
            .clamp(1, MAX_BACKFILL_LIMIT);
        let (slots, truncated) =
            cron_slots_between(&cron_schedule, request.from, request.to, limit)?;
        let mut response = BackfillResponse {
            trigger_id,
            workflow_id: trigger.workflow_id,
            already_fired: 0,
            fired: 0,
            truncated,
            dry_run: request.dry_run,
            run_ids: Vec::new(),
            slots: slots.clone(),
        };
        if request.dry_run {
            response.fired = slots.len() as i64;
            return Ok((response, Vec::new()));
        }

        let now = Utc::now();
        let mut tx = self.pool().begin().await?;
        if workflow_vm.snapshot.id != Some(trigger.workflow_id) {
            return Err(crate::errors::WORKFLOW_VM_CORRUPT_STATE
                .error("backfill module snapshot names a different workflow"));
        }
        let workflow_snapshot = &workflow_vm.snapshot;

        let mut runs = Vec::new();
        for slot in &slots {
            // a slot the loop already fired keeps its original run: the firing row is the same
            // uniqueness gate the trigger loop claims through, so a backfill can never double-run.
            if !self
                .claim_firing_slot(
                    &mut tx,
                    trigger_id,
                    &slot.timestamp().to_string(),
                    "backfill",
                    FiringOutcome::Fired,
                    now,
                )
                .await?
            {
                response.already_fired += 1;
                continue;
            }
            let run = self
                .insert_trigger_run(
                    &mut tx,
                    &trigger,
                    workflow_snapshot,
                    "backfill",
                    *slot,
                    now,
                    &workflow_vm.module,
                )
                .await?;
            response.fired += 1;
            response.run_ids.push(run.id);
            runs.push(run);
        }
        tx.commit().await?;

        Ok((response, runs))
    }

    async fn fetch_freeze_windows(
        &self,
        org_id: Option<Uuid>,
    ) -> Result<Vec<FreezeWindow>, SendableError> {
        // an org listing includes the platform-wide windows, because those are what is actually
        // freezing that org's schedules.
        let sql = match org_id {
            Some(_) => format!(
                "SELECT {FREEZE_WINDOW_COLUMNS} FROM freeze_windows WHERE org_id = ? OR org_id IS NULL ORDER BY starts_at DESC"
            ),
            None => format!(
                "SELECT {FREEZE_WINDOW_COLUMNS} FROM freeze_windows ORDER BY starts_at DESC"
            ),
        };
        let sql = self.render(&sql);
        let mut query = sqlx::query(&sql);
        if let Some(org_id) = org_id {
            query = query.bind(org_id);
        }
        let rows = query.fetch_all(self.pool()).await?;
        Ok(rows.iter().map(mappers::row_to_freeze_window).collect())
    }

    async fn fetch_freeze_window(
        &self,
        window_id: Uuid,
    ) -> Result<Option<FreezeWindow>, SendableError> {
        let columns = FREEZE_WINDOW_COLUMNS;
        let row = sqlx::query(&self.render(&format!(
            "SELECT {columns} FROM freeze_windows WHERE id = ?"
        )))
        .bind(window_id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.as_ref().map(mappers::row_to_freeze_window))
    }

    async fn fetch_active_freeze_windows(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Vec<FreezeWindow>, SendableError> {
        let rows = sqlx::query(&self.render(&format!(
            "SELECT {FREEZE_WINDOW_COLUMNS} FROM freeze_windows WHERE enabled = {} AND starts_at <= ? AND ends_at > ? ORDER BY ends_at",
            self.dialect().bool_true(),
        )))
        .bind(now.timestamp())
        .bind(now.timestamp())
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_freeze_window).collect())
    }

    async fn create_freeze_window(
        &self,
        window: &NewFreezeWindow,
    ) -> Result<FreezeWindow, SendableError> {
        let id = Uuid::now_v7();
        let now = Utc::now().timestamp();
        sqlx::query(&self.render(&format!(
            "INSERT INTO freeze_windows ({FREEZE_WINDOW_COLUMNS}) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )))
        .bind(id)
        .bind(window.org_id)
        .bind(window.workflow_id)
        .bind(window.name.as_str())
        .bind(window.reason.clone())
        .bind(window.starts_at.timestamp())
        .bind(window.ends_at.timestamp())
        .bind(window.enabled)
        .bind(now)
        .bind(now)
        .execute(self.pool())
        .await?;
        let row = sqlx::query(&self.render(&format!(
            "SELECT {FREEZE_WINDOW_COLUMNS} FROM freeze_windows WHERE id = ?"
        )))
        .bind(id)
        .fetch_one(self.pool())
        .await?;
        Ok(mappers::row_to_freeze_window(&row))
    }

    async fn update_freeze_window(
        &self,
        window_id: Uuid,
        window: &NewFreezeWindow,
    ) -> Result<Option<FreezeWindow>, SendableError> {
        let result = sqlx::query(&self.render(
            "UPDATE freeze_windows SET org_id = ?, workflow_id = ?, name = ?, reason = ?, starts_at = ?, ends_at = ?, enabled = ?, updated_at = ? WHERE id = ?",
        ))
        .bind(window.org_id)
        .bind(window.workflow_id)
        .bind(window.name.as_str())
        .bind(window.reason.clone())
        .bind(window.starts_at.timestamp())
        .bind(window.ends_at.timestamp())
        .bind(window.enabled)
        .bind(Utc::now().timestamp())
        .bind(window_id)
        .execute(self.pool())
        .await?;
        if result.affected() == 0 {
            return Ok(None);
        }

        let row = sqlx::query(&self.render(&format!(
            "SELECT {FREEZE_WINDOW_COLUMNS} FROM freeze_windows WHERE id = ?"
        )))
        .bind(window_id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.as_ref().map(mappers::row_to_freeze_window))
    }

    async fn delete_freeze_window(&self, window_id: Uuid) -> Result<bool, SendableError> {
        Ok(retry_delete(|| async {
            sqlx::query(&self.render("DELETE FROM freeze_windows WHERE id = ?"))
                .bind(window_id)
                .execute(self.pool())
                .await
                .map(|result| result.affected() > 0)
        })
        .await?)
    }
}
