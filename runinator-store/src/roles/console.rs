//! the rexrap console: sessions, their cells, and the scope those cells share.
//!
//! one of the role traits `DatabaseImpl` composes. bound on this directly when a caller only needs
//! this slice of the store.
//!
//! the scope is stored rather than accumulated in memory. a console session outlives any one
//! request, and a scope held in a replica's process would give different answers depending on which
//! replica served the cell.

use std::future::Future;

use uuid::Uuid;

use runinator_models::{
    console::{
        ConsoleBinding, ConsoleCell, ConsoleCellKind, ConsoleCellStatus, ConsoleFunction,
        ConsoleSession, NewConsoleCell, NewConsoleFunction,
    },
    errors::SendableError,
    value::Value,
};

/// Persistence for the rexrap console.
pub trait ConsoleStore: Send + Sync + 'static {
    /// Create a session.
    fn create_console_session(
        &self,
        org_id: Option<Uuid>,
        name: &str,
        created_by: Option<Uuid>,
    ) -> impl Future<Output = Result<ConsoleSession, SendableError>> + Send;

    /// Fetch every session, newest first.
    fn fetch_console_sessions(
        &self,
    ) -> impl Future<Output = Result<Vec<ConsoleSession>, SendableError>> + Send;

    /// Fetch one session.
    fn fetch_console_session(
        &self,
        session_id: Uuid,
    ) -> impl Future<Output = Result<Option<ConsoleSession>, SendableError>> + Send;

    /// Rename a session.
    fn rename_console_session(
        &self,
        session_id: Uuid,
        name: &str,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    /// Delete a session and everything under it.
    fn delete_console_session(
        &self,
        session_id: Uuid,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    /// Append or replace a cell. A `position` in the request replaces the cell there; omitting it
    /// appends.
    fn upsert_console_cell(
        &self,
        session_id: Uuid,
        cell_id: Option<Uuid>,
        cell: &NewConsoleCell,
    ) -> impl Future<Output = Result<ConsoleCell, SendableError>> + Send;

    /// Fetch a session's cells in order.
    fn fetch_console_cells(
        &self,
        session_id: Uuid,
    ) -> impl Future<Output = Result<Vec<ConsoleCell>, SendableError>> + Send;

    /// Fetch one cell.
    fn fetch_console_cell(
        &self,
        cell_id: Uuid,
    ) -> impl Future<Output = Result<Option<ConsoleCell>, SendableError>> + Send;

    /// Record what happened to a cell.
    ///
    /// One call rather than a status write and a result write, because a cell that recorded a
    /// result without its status (or the reverse) is a state no reader knows how to interpret.
    fn record_console_cell_outcome(
        &self,
        cell_id: Uuid,
        kind: Option<ConsoleCellKind>,
        status: ConsoleCellStatus,
        result: Option<&Value>,
        error: Option<&str>,
        workflow_run_id: Option<Uuid>,
    ) -> impl Future<Output = Result<Option<ConsoleCell>, SendableError>> + Send;

    /// Delete one cell. Its binding goes too — a name resolving to a deleted cell's result is a
    /// scope entry nothing can explain.
    fn delete_console_cell(
        &self,
        cell_id: Uuid,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    /// Bind a name in a session's scope, replacing any existing one.
    fn upsert_console_binding(
        &self,
        session_id: Uuid,
        name: &str,
        cell_id: Option<Uuid>,
        value: &Value,
    ) -> impl Future<Output = Result<ConsoleBinding, SendableError>> + Send;

    /// Fetch a session's scope.
    fn fetch_console_bindings(
        &self,
        session_id: Uuid,
    ) -> impl Future<Output = Result<Vec<ConsoleBinding>, SendableError>> + Send;

    /// Drop one name from a session's scope.
    fn delete_console_binding(
        &self,
        session_id: Uuid,
        name: &str,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    /// Fetch the active function library for a session, keyed by its latest successful
    /// publication of each name.
    fn fetch_console_functions(
        &self,
        session_id: Uuid,
    ) -> impl Future<Output = Result<Vec<ConsoleFunction>, SendableError>> + Send;

    /// Replace every definition currently owned by `cell_id`, then publish these candidates. A
    /// published name replaces the session's former owner, implementing latest-successful wins.
    fn replace_console_functions(
        &self,
        session_id: Uuid,
        cell_id: Uuid,
        functions: &[NewConsoleFunction],
    ) -> impl Future<Output = Result<Vec<ConsoleFunction>, SendableError>> + Send;

    /// Mark a function-only cell as successful and publish its definitions as one state
    /// transition. `source` is the text that was validated; if the cell changed while validation
    /// was in flight, nothing is published.
    fn publish_console_library_cell(
        &self,
        cell_id: Uuid,
        source: &str,
        functions: &[NewConsoleFunction],
    ) -> impl Future<Output = Result<Option<ConsoleCell>, SendableError>> + Send;

    /// Atomically settle the scratch run currently owned by a cell, bind its result, and publish
    /// any definitions the completed source declared. A cell edited or replayed onto another run
    /// no longer owns `workflow_run_id`, so it is deliberately left untouched.
    fn settle_console_workflow_succeeded(
        &self,
        cell_id: Uuid,
        workflow_run_id: Uuid,
        binding_name: &str,
        value: &Value,
        functions: &[NewConsoleFunction],
    ) -> impl Future<Output = Result<Option<ConsoleCell>, SendableError>> + Send;

    /// Atomically mark the scratch run currently owned by a cell as failed and clear its binding.
    /// As with success, a stale run is ignored rather than overwriting an edited or replayed cell.
    fn settle_console_workflow_failed(
        &self,
        cell_id: Uuid,
        workflow_run_id: Uuid,
        binding_name: &str,
        error: &str,
    ) -> impl Future<Output = Result<Option<ConsoleCell>, SendableError>> + Send;

    /// Find the cell waiting on a scratch workflow run, so a settled run can be attributed back.
    fn fetch_console_cell_for_run(
        &self,
        workflow_run_id: Uuid,
    ) -> impl Future<Output = Result<Option<ConsoleCell>, SendableError>> + Send;
}
