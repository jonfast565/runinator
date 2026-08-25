//! the rexrap console: sessions, cells, and the scope they share.
//!
//! the `ConsoleStore` half of the generic sql implementation. bodies are written once, over any
//! `SqlBackend`; see `super` for the shared helpers they call.

use super::*;

/// every column `mappers::row_to_console_session` reads.
const CONSOLE_SESSION_COLUMNS: &str = "id, org_id, name, created_by, created_at, updated_at";
const CONSOLE_CELL_COLUMNS: &str = "id, session_id, position, label, source, kind, status, result, \
                                    error, workflow_run_id, created_at, updated_at";
const CONSOLE_BINDING_COLUMNS: &str =
    "id, session_id, name, cell_id, value, created_at, updated_at";
const CONSOLE_FUNCTION_COLUMNS: &str =
    "id, session_id, cell_id, name, is_task, source, created_at, updated_at";

/// Replace a cell's published definitions while another console transition already owns the
/// transaction. Keeping this beside the role implementation makes library publication and scratch
/// completion use the exact same ownership semantics.
async fn replace_console_functions_in_transaction<B>(
    store: &SqlStore<B>,
    tx: &mut sqlx::Transaction<'_, B::Db>,
    session_id: Uuid,
    cell_id: Uuid,
    functions: &[NewConsoleFunction],
    now: i64,
) -> Result<(), SendableError>
where
    B: SqlBackend,
    for<'q> i64: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> bool: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> &'q str: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Uuid: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> <B::Db as Database>::Arguments<'q>: IntoArguments<'q, B::Db>,
    for<'c> &'c mut <B::Db as Database>::Connection: Executor<'c, Database = B::Db>,
{
    sqlx::query(&store.render("DELETE FROM console_functions WHERE cell_id = ?"))
        .bind(cell_id)
        .execute(&mut **tx)
        .await?;

    let mut functions = functions.iter().collect::<Vec<_>>();
    functions.sort_by(|left, right| left.name.cmp(&right.name));
    let conflict = store.dialect().on_conflict_update(
        "session_id, name",
        &["cell_id", "is_task", "source", "updated_at"],
    );
    for function in functions {
        sqlx::query(&store.render(&format!(
            "INSERT INTO console_functions ({CONSOLE_FUNCTION_COLUMNS}) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?) {conflict}"
        )))
        .bind(Uuid::new_v4())
        .bind(session_id)
        .bind(cell_id)
        .bind(function.name.as_str())
        .bind(function.is_task)
        .bind(function.source.as_str())
        .bind(now)
        .bind(now)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

impl<B> ConsoleStore for SqlStore<B>
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
    async fn create_console_session(
        &self,
        org_id: Option<Uuid>,
        name: &str,
        created_by: Option<Uuid>,
    ) -> Result<ConsoleSession, SendableError> {
        let id = Uuid::new_v4();
        let now = Utc::now().timestamp();
        sqlx::query(&self.render(&format!(
            "INSERT INTO console_sessions ({CONSOLE_SESSION_COLUMNS}) VALUES (?, ?, ?, ?, ?, ?)"
        )))
        .bind(id)
        .bind(org_id)
        .bind(name)
        .bind(created_by)
        .bind(now)
        .bind(now)
        .execute(self.pool())
        .await?;
        self.fetch_console_session(id)
            .await?
            .ok_or_else(|| -> SendableError { "console session vanished after insert".into() })
    }

    async fn fetch_console_sessions(&self) -> Result<Vec<ConsoleSession>, SendableError> {
        let rows = sqlx::query(&self.render(&format!(
            "SELECT {CONSOLE_SESSION_COLUMNS} FROM console_sessions ORDER BY updated_at DESC"
        )))
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_console_session).collect())
    }

    async fn fetch_console_session(
        &self,
        session_id: Uuid,
    ) -> Result<Option<ConsoleSession>, SendableError> {
        let row = sqlx::query(&self.render(&format!(
            "SELECT {CONSOLE_SESSION_COLUMNS} FROM console_sessions WHERE id = ?"
        )))
        .bind(session_id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|row| mappers::row_to_console_session(&row)))
    }

    async fn rename_console_session(
        &self,
        session_id: Uuid,
        name: &str,
    ) -> Result<bool, SendableError> {
        let result = sqlx::query(
            &self.render("UPDATE console_sessions SET name = ?, updated_at = ? WHERE id = ?"),
        )
        .bind(name)
        .bind(Utc::now().timestamp())
        .bind(session_id)
        .execute(self.pool())
        .await?;
        Ok(result.affected() > 0)
    }

    async fn delete_console_session(&self, session_id: Uuid) -> Result<bool, SendableError> {
        // children explicitly, deepest first, inside one transaction. the schema declares cascades,
        // but mysql 8 silently discards an inline `REFERENCES` clause, so relying on the cascade
        // would orphan rows on one engine and not the other.
        let mut tx = self.pool().begin().await?;
        // Take cell locks before their bindings/functions. A concurrent settlement takes this
        // same order, so deleting a session either follows a completed run or makes that run's
        // compare-and-set miss after deletion; it cannot interleave a partial library update.
        sqlx::query(
            &self.render(
                "UPDATE console_cells SET updated_at = updated_at + 1 WHERE session_id = ?",
            ),
        )
        .bind(session_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query(&self.render("DELETE FROM console_bindings WHERE session_id = ?"))
            .bind(session_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query(&self.render("DELETE FROM console_functions WHERE session_id = ?"))
            .bind(session_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query(&self.render("DELETE FROM console_cells WHERE session_id = ?"))
            .bind(session_id)
            .execute(&mut *tx)
            .await?;
        let result = sqlx::query(&self.render("DELETE FROM console_sessions WHERE id = ?"))
            .bind(session_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(result.affected() > 0)
    }

    async fn upsert_console_cell(
        &self,
        session_id: Uuid,
        cell_id: Option<Uuid>,
        cell: &NewConsoleCell,
    ) -> Result<ConsoleCell, SendableError> {
        let now = Utc::now().timestamp();
        if let Some(cell_id) = cell_id {
            // editing a cell clears its previous outcome: a result left beside changed source is a
            // stale answer presented as a current one. It also removes definitions in the same
            // transaction, so readers never see an edited owner beside its old library entries.
            let mut tx = self.pool().begin().await?;
            sqlx::query(&self.render(
                "UPDATE console_cells SET source = ?, label = ?, kind = NULL, status = ?, \
                 result = NULL, error = NULL, workflow_run_id = NULL, updated_at = ? WHERE id = ?",
            ))
            .bind(cell.source.as_str())
            .bind(cell.label.clone())
            .bind(ConsoleCellStatus::Idle.as_str())
            .bind(now)
            .bind(cell_id)
            .execute(&mut *tx)
            .await?;
            // Completion takes the same lock order (cell, then functions). Keeping every
            // transition in that order avoids a two-row deadlock when an edit meets a settle.
            sqlx::query(&self.render("DELETE FROM console_functions WHERE cell_id = ?"))
                .bind(cell_id)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            return self
                .fetch_console_cell(cell_id)
                .await?
                .ok_or_else(|| -> SendableError { "console cell not found".into() });
        }

        let position = match cell.position {
            Some(position) => position,
            None => {
                let row: Option<(i64,)> = sqlx::query_as(&self.render(
                    "SELECT COALESCE(MAX(position), -1) FROM console_cells WHERE session_id = ?",
                ))
                .bind(session_id)
                .fetch_optional(self.pool())
                .await?;
                row.map(|(max,)| max + 1).unwrap_or(0)
            }
        };
        let id = Uuid::new_v4();
        sqlx::query(&self.render(&format!(
            "INSERT INTO console_cells ({CONSOLE_CELL_COLUMNS}) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )))
        .bind(id)
        .bind(session_id)
        .bind(position)
        .bind(cell.label.clone())
        .bind(cell.source.as_str())
        .bind(None::<String>)
        .bind(ConsoleCellStatus::Idle.as_str())
        .bind(None::<String>)
        .bind(None::<String>)
        .bind(None::<Uuid>)
        .bind(now)
        .bind(now)
        .execute(self.pool())
        .await?;
        self.fetch_console_cell(id)
            .await?
            .ok_or_else(|| -> SendableError { "console cell vanished after insert".into() })
    }

    async fn fetch_console_cells(
        &self,
        session_id: Uuid,
    ) -> Result<Vec<ConsoleCell>, SendableError> {
        let rows = sqlx::query(&self.render(&format!(
            "SELECT {CONSOLE_CELL_COLUMNS} FROM console_cells WHERE session_id = ? ORDER BY position ASC"
        )))
        .bind(session_id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_console_cell).collect())
    }

    async fn fetch_console_cell(
        &self,
        cell_id: Uuid,
    ) -> Result<Option<ConsoleCell>, SendableError> {
        let row = sqlx::query(&self.render(&format!(
            "SELECT {CONSOLE_CELL_COLUMNS} FROM console_cells WHERE id = ?"
        )))
        .bind(cell_id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|row| mappers::row_to_console_cell(&row)))
    }

    async fn record_console_cell_outcome(
        &self,
        cell_id: Uuid,
        kind: Option<ConsoleCellKind>,
        status: ConsoleCellStatus,
        result: Option<&Value>,
        error: Option<&str>,
        workflow_run_id: Option<Uuid>,
    ) -> Result<Option<ConsoleCell>, SendableError> {
        let encoded = result
            .map(|value| serde_json::to_string(value))
            .transpose()?;
        sqlx::query(&self.render(
            "UPDATE console_cells SET kind = ?, status = ?, result = ?, error = ?, \
             workflow_run_id = ?, updated_at = ? WHERE id = ?",
        ))
        .bind(kind.map(|kind| kind.as_str().to_string()))
        .bind(status.as_str())
        .bind(encoded)
        .bind(error.map(str::to_string))
        .bind(workflow_run_id)
        .bind(Utc::now().timestamp())
        .bind(cell_id)
        .execute(self.pool())
        .await?;
        self.fetch_console_cell(cell_id).await
    }

    async fn delete_console_cell(&self, cell_id: Uuid) -> Result<bool, SendableError> {
        let mut tx = self.pool().begin().await?;
        // Lock the cell before child rows. This matches edit and completion, preventing a delete
        // from deadlocking with a workflow that is simultaneously settling its result/library.
        let locked = sqlx::query(
            &self.render("UPDATE console_cells SET updated_at = updated_at + 1 WHERE id = ?"),
        )
        .bind(cell_id)
        .execute(&mut *tx)
        .await?;
        if locked.affected() == 0 {
            return Ok(false);
        }
        // the binding goes with it: a name resolving to a deleted cell's result is a scope entry
        // nothing can explain or reproduce.
        sqlx::query(&self.render("DELETE FROM console_bindings WHERE cell_id = ?"))
            .bind(cell_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query(&self.render("DELETE FROM console_functions WHERE cell_id = ?"))
            .bind(cell_id)
            .execute(&mut *tx)
            .await?;
        let result = sqlx::query(&self.render("DELETE FROM console_cells WHERE id = ?"))
            .bind(cell_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(result.affected() > 0)
    }

    async fn upsert_console_binding(
        &self,
        session_id: Uuid,
        name: &str,
        cell_id: Option<Uuid>,
        value: &Value,
    ) -> Result<ConsoleBinding, SendableError> {
        let encoded = serde_json::to_string(value)?;
        let now = Utc::now().timestamp();
        let existing: Option<(Uuid,)> = sqlx::query_as(
            &self.render("SELECT id FROM console_bindings WHERE session_id = ? AND name = ?"),
        )
        .bind(session_id)
        .bind(name)
        .fetch_optional(self.pool())
        .await?;

        // re-running a cell replaces what its name resolves to rather than adding a second row the
        // scope builder would have to choose between.
        match existing {
            Some((id,)) => {
                sqlx::query(&self.render(
                    "UPDATE console_bindings SET cell_id = ?, value = ?, updated_at = ? WHERE id = ?",
                ))
                .bind(cell_id)
                .bind(encoded.as_str())
                .bind(now)
                .bind(id)
                .execute(self.pool())
                .await?;
            }
            None => {
                sqlx::query(&self.render(&format!(
                    "INSERT INTO console_bindings ({CONSOLE_BINDING_COLUMNS}) VALUES (?, ?, ?, ?, ?, ?, ?)"
                )))
                .bind(Uuid::new_v4())
                .bind(session_id)
                .bind(name)
                .bind(cell_id)
                .bind(encoded.as_str())
                .bind(now)
                .bind(now)
                .execute(self.pool())
                .await?;
            }
        }

        let row = sqlx::query(&self.render(&format!(
            "SELECT {CONSOLE_BINDING_COLUMNS} FROM console_bindings WHERE session_id = ? AND name = ?"
        )))
        .bind(session_id)
        .bind(name)
        .fetch_one(self.pool())
        .await?;
        Ok(mappers::row_to_console_binding(&row))
    }

    async fn fetch_console_bindings(
        &self,
        session_id: Uuid,
    ) -> Result<Vec<ConsoleBinding>, SendableError> {
        let rows = sqlx::query(&self.render(&format!(
            "SELECT {CONSOLE_BINDING_COLUMNS} FROM console_bindings WHERE session_id = ? ORDER BY name ASC"
        )))
        .bind(session_id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_console_binding).collect())
    }

    async fn delete_console_binding(
        &self,
        session_id: Uuid,
        name: &str,
    ) -> Result<bool, SendableError> {
        let result = sqlx::query(
            &self.render("DELETE FROM console_bindings WHERE session_id = ? AND name = ?"),
        )
        .bind(session_id)
        .bind(name)
        .execute(self.pool())
        .await?;
        Ok(result.affected() > 0)
    }

    async fn fetch_console_functions(
        &self,
        session_id: Uuid,
    ) -> Result<Vec<ConsoleFunction>, SendableError> {
        let rows = sqlx::query(&self.render(&format!(
            "SELECT {CONSOLE_FUNCTION_COLUMNS} FROM console_functions \
             WHERE session_id = ? ORDER BY name ASC"
        )))
        .bind(session_id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_console_function).collect())
    }

    async fn replace_console_functions(
        &self,
        session_id: Uuid,
        cell_id: Uuid,
        functions: &[NewConsoleFunction],
    ) -> Result<Vec<ConsoleFunction>, SendableError> {
        let now = Utc::now().timestamp();
        let mut tx = self.pool().begin().await?;
        // A re-run replaces all names this cell formerly owned. Definitions it no longer contains
        // do not resurrect an older owner; deleting is the documented lifecycle rule.
        replace_console_functions_in_transaction(
            self, &mut tx, session_id, cell_id, functions, now,
        )
        .await?;
        tx.commit().await?;
        self.fetch_console_functions(session_id).await
    }

    async fn publish_console_library_cell(
        &self,
        cell_id: Uuid,
        source: &str,
        functions: &[NewConsoleFunction],
    ) -> Result<Option<ConsoleCell>, SendableError> {
        let now = Utc::now().timestamp();
        let mut tx = self.pool().begin().await?;
        // The source check is the library-cell equivalent of matching a workflow run id below.
        // If an editor saved new text while semantic validation was in flight, that validation
        // must not publish definitions for the old text.
        let updated = sqlx::query(&self.render(
            "UPDATE console_cells SET kind = ?, status = ?, result = NULL, error = NULL, \
             workflow_run_id = NULL, updated_at = ? WHERE id = ? AND source = ?",
        ))
        .bind(ConsoleCellKind::Library.as_str())
        .bind(ConsoleCellStatus::Succeeded.as_str())
        .bind(now)
        .bind(cell_id)
        .bind(source)
        .execute(&mut *tx)
        .await?;
        if updated.affected() == 0 {
            return Ok(None);
        }
        let (session_id,): (Uuid,) =
            sqlx::query_as(&self.render("SELECT session_id FROM console_cells WHERE id = ?"))
                .bind(cell_id)
                .fetch_one(&mut *tx)
                .await?;
        replace_console_functions_in_transaction(
            self, &mut tx, session_id, cell_id, functions, now,
        )
        .await?;
        tx.commit().await?;
        self.fetch_console_cell(cell_id).await
    }

    async fn settle_console_workflow_succeeded(
        &self,
        cell_id: Uuid,
        workflow_run_id: Uuid,
        binding_name: &str,
        value: &Value,
        functions: &[NewConsoleFunction],
    ) -> Result<Option<ConsoleCell>, SendableError> {
        let now = Utc::now().timestamp();
        let encoded = serde_json::to_string(value)?;
        let mut tx = self.pool().begin().await?;
        // This must be first: it acts as the compare-and-set that establishes this completion is
        // still current, while also locking the cell row until the binding and function library
        // move with it.
        let updated = sqlx::query(&self.render(
            "UPDATE console_cells SET kind = ?, status = ?, result = ?, error = NULL, \
             workflow_run_id = ?, updated_at = ? WHERE id = ? AND workflow_run_id = ? AND status = ?",
        ))
        .bind(ConsoleCellKind::Workflow.as_str())
        .bind(ConsoleCellStatus::Succeeded.as_str())
        .bind(encoded.as_str())
        .bind(workflow_run_id)
        .bind(now)
        .bind(cell_id)
        .bind(workflow_run_id)
        .bind(ConsoleCellStatus::Running.as_str())
        .execute(&mut *tx)
        .await?;
        if updated.affected() == 0 {
            return Ok(None);
        }
        let (session_id,): (Uuid,) =
            sqlx::query_as(&self.render("SELECT session_id FROM console_cells WHERE id = ?"))
                .bind(cell_id)
                .fetch_one(&mut *tx)
                .await?;
        let binding_conflict = self
            .dialect()
            .on_conflict_update("session_id, name", &["cell_id", "value", "updated_at"]);
        sqlx::query(&self.render(&format!(
            "INSERT INTO console_bindings ({CONSOLE_BINDING_COLUMNS}) VALUES (?, ?, ?, ?, ?, ?, ?) {binding_conflict}"
        )))
        .bind(Uuid::new_v4())
        .bind(session_id)
        .bind(binding_name)
        .bind(cell_id)
        .bind(encoded.as_str())
        .bind(now)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        replace_console_functions_in_transaction(
            self, &mut tx, session_id, cell_id, functions, now,
        )
        .await?;
        tx.commit().await?;
        self.fetch_console_cell(cell_id).await
    }

    async fn settle_console_workflow_failed(
        &self,
        cell_id: Uuid,
        workflow_run_id: Uuid,
        binding_name: &str,
        error: &str,
    ) -> Result<Option<ConsoleCell>, SendableError> {
        let now = Utc::now().timestamp();
        let mut tx = self.pool().begin().await?;
        let updated = sqlx::query(&self.render(
            "UPDATE console_cells SET kind = ?, status = ?, result = NULL, error = ?, \
             workflow_run_id = ?, updated_at = ? WHERE id = ? AND workflow_run_id = ? AND status = ?",
        ))
        .bind(ConsoleCellKind::Workflow.as_str())
        .bind(ConsoleCellStatus::Failed.as_str())
        .bind(error)
        .bind(workflow_run_id)
        .bind(now)
        .bind(cell_id)
        .bind(workflow_run_id)
        .bind(ConsoleCellStatus::Running.as_str())
        .execute(&mut *tx)
        .await?;
        if updated.affected() == 0 {
            return Ok(None);
        }
        let (session_id,): (Uuid,) =
            sqlx::query_as(&self.render("SELECT session_id FROM console_cells WHERE id = ?"))
                .bind(cell_id)
                .fetch_one(&mut *tx)
                .await?;
        // A later cell may now own this name. Only clear the binding this cell itself created.
        sqlx::query(&self.render(
            "DELETE FROM console_bindings WHERE session_id = ? AND name = ? AND cell_id = ?",
        ))
        .bind(session_id)
        .bind(binding_name)
        .bind(cell_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        self.fetch_console_cell(cell_id).await
    }

    async fn fetch_console_cell_for_run(
        &self,
        workflow_run_id: Uuid,
    ) -> Result<Option<ConsoleCell>, SendableError> {
        let row = sqlx::query(&self.render(&format!(
            "SELECT {CONSOLE_CELL_COLUMNS} FROM console_cells WHERE workflow_run_id = ?"
        )))
        .bind(workflow_run_id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|row| mappers::row_to_console_cell(&row)))
    }
}
