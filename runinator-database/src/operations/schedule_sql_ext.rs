#[allow(unused_imports)]
use super::*;

pub(super) trait ScheduleSqlExt: SqlBackend {
    /// how many of a workflow's runs have not reached a terminal state.
    async fn active_run_count(
        &self,
        conn: &mut <Self::Db as Database>::Connection,
        workflow_id: Uuid,
    ) -> Result<i64, SendableError>;

    /// set every non-terminal run of a workflow to `canceled`, returning the ids. the caller still
    /// has to tell the workers holding those runs' actions; this only settles durable state.
    async fn cancel_active_runs(
        &self,
        conn: &mut <Self::Db as Database>::Connection,
        workflow_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<Vec<Uuid>, SendableError>;

    /// claim a slot by recording its firing. `false` means another replica already claimed it.
    async fn claim_firing_slot(
        &self,
        conn: &mut <Self::Db as Database>::Connection,
        trigger_id: Uuid,
        fire_key: &str,
        scheduler_id: &str,
        outcome: FiringOutcome,
        now: DateTime<Utc>,
    ) -> Result<bool, SendableError>;

    /// create the run for a claimed slot and point the firing row at it.
    async fn insert_trigger_run(
        &self,
        conn: &mut <Self::Db as Database>::Connection,
        trigger: &WorkflowTrigger,
        snapshot: &WorkflowDefinition,
        context: TriggerRunContext<'_>,
        module: &WorkflowModule,
    ) -> Result<WorkflowRun, SendableError>;
}

impl<B> ScheduleSqlExt for B
where
    B: SqlBackend,
    for<'q> i64: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> bool: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Uuid: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> &'q str: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> String: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Option<Uuid>: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Option<String>: Encode<'q, B::Db> + Type<B::Db>,
    <B::Db as Database>::Arguments: IntoArguments<B::Db>,
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
    for<'c> &'c mut <B::Db as Database>::Connection: Executor<'c, Database = B::Db>,
    <B::Db as Database>::QueryResult: RowsAffected,
{
    async fn active_run_count(
        &self,
        conn: &mut <Self::Db as Database>::Connection,
        workflow_id: Uuid,
    ) -> Result<i64, SendableError> {
        let row = sqlx::query(&self.render(&format!(
            "SELECT COUNT(*) AS active FROM workflow_runs WHERE workflow_id = ? AND status NOT IN ({})",
            status_list(&WorkflowStatus::TERMINAL),
        )))
        .bind(workflow_id)
        .fetch_one(conn)
        .await?;
        Ok(row.get::<i64, _>("active"))
    }

    async fn cancel_active_runs(
        &self,
        conn: &mut <Self::Db as Database>::Connection,
        workflow_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<Vec<Uuid>, SendableError> {
        let terminal = status_list(&WorkflowStatus::TERMINAL);
        let rows = sqlx::query(&self.render(&format!(
            "SELECT id FROM workflow_runs WHERE workflow_id = ? AND status NOT IN ({terminal})"
        )))
        .bind(workflow_id)
        .fetch_all(&mut *conn)
        .await?;
        let ids: Vec<Uuid> = rows.iter().map(|row| row.get::<Uuid, _>("id")).collect();
        if ids.is_empty() {
            return Ok(ids);
        }

        sqlx::query(&self.render(&format!(
            "UPDATE workflow_runs SET status = ?, finished_at = ?, message = ? WHERE workflow_id = ? AND status NOT IN ({terminal})"
        )))
        .bind(WorkflowStatus::Canceled.as_str())
        .bind(now.timestamp())
        .bind("Canceled by a newer run of this workflow (cancel_previous concurrency policy)")
        .bind(workflow_id)
        .execute(conn)
        .await?;

        Ok(ids)
    }

    async fn claim_firing_slot(
        &self,
        conn: &mut <Self::Db as Database>::Connection,
        trigger_id: Uuid,
        fire_key: &str,
        scheduler_id: &str,
        outcome: FiringOutcome,
        now: DateTime<Utc>,
    ) -> Result<bool, SendableError> {
        let sql = self.render(&self.dialect().insert_ignore(
            "workflow_trigger_firings",
            "id, trigger_id, fire_key, scheduler_id, outcome, created_at",
            "?, ?, ?, ?, ?, ?",
            "trigger_id, fire_key",
            None,
        ));
        let insert = sqlx::query(&sql)
            .bind(Uuid::now_v7())
            .bind(trigger_id)
            .bind(fire_key)
            .bind(scheduler_id)
            .bind(outcome.as_str())
            .bind(now.timestamp())
            .execute(conn)
            .await?;
        Ok(insert.affected() > 0)
    }

    async fn insert_trigger_run(
        &self,
        conn: &mut <Self::Db as Database>::Connection,
        trigger: &WorkflowTrigger,
        snapshot: &WorkflowDefinition,
        context: TriggerRunContext<'_>,
        module: &WorkflowModule,
    ) -> Result<WorkflowRun, SendableError> {
        let Some(trigger_id) = trigger.id else {
            return Err(crate::errors::TRIGGER_MISSING_ID.bare());
        };
        let new_run_id = Uuid::now_v7();
        let snapshot_json = serde_json::to_string(snapshot)?;
        if !module.is_supported() || module.instructions.is_empty() {
            return Err(crate::errors::WORKFLOW_VM_CORRUPT_STATE
                .error("cannot fire a trigger with an incompatible workflow module"));
        }
        let parameter_value = trigger.trigger_parameters();
        let parameters = parameter_value.to_string();
        let state =
            WorkflowExecutionState::from_state(&trigger.trigger_state_for_slot(context.slot));
        let insert_sql = "INSERT INTO workflow_runs (id, workflow_id, workflow_snapshot, status, active_node_id, parameters, created_at, name, trigger_source_kind, trigger_actor_type, trigger_actor_replica_id, trigger_actor_display_name, trigger_request_host, trigger_request_ip, trigger_metadata) VALUES (?, ?, ?, ?, NULL, ?, ?, NULL, ?, ?, NULL, ?, NULL, NULL, ?)";
        sqlx::query(&self.render(insert_sql))
            .bind(new_run_id)
            .bind(trigger.workflow_id)
            .bind(&snapshot_json)
            .bind(WorkflowStatus::Queued.as_str())
            .bind(&parameters)
            .bind(context.now.timestamp())
            .bind("cron")
            .bind("replica")
            .bind(context.scheduler_id)
            .bind(trigger.metadata.to_string())
            .execute(&mut *conn)
            .await?;
        execution_state_sql::write(self, conn, new_run_id, &state, false).await?;
        let mut continuation = WorkflowContinuation::start(new_run_id, module.version);
        continuation.locals.insert("input".into(), parameter_value);
        sqlx::query(&self.render(
            "INSERT INTO workflow_vm_modules (workflow_run_id, version, module_json, created_at) VALUES (?, ?, ?, ?)",
        ))
        .bind(new_run_id)
        .bind(i64::from(module.version))
        .bind(serde_json::to_string(module)?)
        .bind(context.now.timestamp())
        .execute(&mut *conn)
        .await?;
        sqlx::query(&self.render(
            "INSERT INTO workflow_continuations (id, workflow_run_id, module_version, continuation_json, status, version, ready_at, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        ))
        .bind(continuation.id)
        .bind(new_run_id)
        .bind(i64::from(continuation.module_version))
        .bind(serde_json::to_string(&continuation)?)
        .bind("runnable")
        .bind(continuation.revision as i64)
        .bind(context.now.timestamp())
        .bind(context.now.timestamp())
        .bind(context.now.timestamp())
        .execute(&mut *conn)
        .await?;
        let entry = WorkflowJournalEntry::Entered {
            continuation_id: continuation.id,
            instruction_pointer: 0,
        };
        sqlx::query(&self.render(
            "INSERT INTO workflow_journal_entries (id, version, workflow_run_id, sequence, continuation_id, effect_id, entry_json, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        ))
        .bind(Uuid::now_v7())
        .bind(i64::from(WORKFLOW_JOURNAL_VERSION))
        .bind(new_run_id)
        .bind(0_i64)
        .bind(Some(continuation.id))
        .bind(Option::<Uuid>::None)
        .bind(serde_json::to_string(&entry)?)
        .bind(context.now.timestamp())
        .execute(&mut *conn)
        .await?;
        let run_row = sqlx::query(&self.render(&format!(
            "SELECT {WORKFLOW_RUN_COLUMNS} FROM workflow_runs WHERE id = ?"
        )))
        .bind(new_run_id)
        .fetch_one(&mut *conn)
        .await?;
        let mut run = mappers::row_to_workflow_run(&run_row);
        run.execution_state = state;

        sqlx::query(&self.render(
            "UPDATE workflow_trigger_firings SET workflow_run_id = ? WHERE trigger_id = ? AND fire_key = ?",
        ))
        .bind(run.id)
        .bind(trigger_id)
        .bind(context.slot.timestamp().to_string())
        .execute(conn)
        .await?;

        Ok(run)
    }
}
