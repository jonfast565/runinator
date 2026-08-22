//! running a console cell.
//!
//! the classifier says whether a cell is pure; this decides what to do about it. a pure cell is
//! evaluated in process and settles in the same request. anything else is compiled into a scratch
//! workflow and started as a run, and the cell is left `Running` until that run settles.
//!
//! the two halves deliberately share their tail: both record an outcome through
//! [`settle_cell`], and both bind the result into the session's scope the same way. a console
//! where a pure cell and an effectful one produced differently-shaped results would be a console
//! where `cells.a` means something different depending on how `a` happened to be written.

use runinator_console::{CellKind, ConsoleContext, cell_binding_name, scratch_workflow_name};
use runinator_models::console::{
    CONSOLE_MANAGED_BY, ConsoleBinding, ConsoleCell, ConsoleCellKind, ConsoleCellStatus,
    ConsoleSession, ConsoleSessionDetail, NewConsoleCell,
};
use runinator_models::errors::SendableError;
use runinator_models::replicas::{TriggerActorType, TriggerSourceKind, WorkflowRunProvenance};
use runinator_models::revisions::{RevisionAuthor, RevisionSource};
use runinator_models::value::Value;
use runinator_models::workflows::WorkflowRun;
use runinator_store::{
    RuntimeStore,
    roles::{
        ConsoleStore, DefinitionStore, FunctionStore, NotificationStore, ScheduleStore,
        WorkflowVmStore,
    },
};
use uuid::Uuid;

use crate::errors::{CONSOLE_CELL_NOT_FOUND, CONSOLE_SESSION_NOT_FOUND};

/// what running a cell produced.
#[derive(Debug, Clone)]
pub struct CellOutcome {
    pub cell: ConsoleCell,
    /// present when the cell became a scratch workflow, so the caller can poll or stream it.
    pub run: Option<WorkflowRun>,
}

pub async fn create_session<T: ConsoleStore>(
    db: &T,
    org_id: Option<Uuid>,
    name: &str,
    created_by: Option<Uuid>,
) -> Result<ConsoleSession, SendableError> {
    db.create_console_session(org_id, name, created_by).await
}

pub async fn fetch_sessions<T: ConsoleStore>(db: &T) -> Result<Vec<ConsoleSession>, SendableError> {
    db.fetch_console_sessions().await
}

/// one session with its cells and scope.
pub async fn fetch_session_detail<T: ConsoleStore>(
    db: &T,
    session_id: Uuid,
) -> Result<Option<ConsoleSessionDetail>, SendableError> {
    let Some(session) = db.fetch_console_session(session_id).await? else {
        return Ok(None);
    };
    Ok(Some(ConsoleSessionDetail {
        session,
        cells: db.fetch_console_cells(session_id).await?,
        bindings: db.fetch_console_bindings(session_id).await?,
    }))
}

pub async fn rename_session<T: ConsoleStore>(
    db: &T,
    session_id: Uuid,
    name: &str,
) -> Result<bool, SendableError> {
    db.rename_console_session(session_id, name).await
}

pub async fn delete_session<T: ConsoleStore>(
    db: &T,
    session_id: Uuid,
) -> Result<bool, SendableError> {
    db.delete_console_session(session_id).await
}

pub async fn upsert_cell<T: ConsoleStore>(
    db: &T,
    session_id: Uuid,
    cell_id: Option<Uuid>,
    cell: &NewConsoleCell,
) -> Result<ConsoleCell, SendableError> {
    if db.fetch_console_session(session_id).await?.is_none() {
        return Err(CONSOLE_SESSION_NOT_FOUND.error(format!("session {session_id} not found")));
    }
    db.upsert_console_cell(session_id, cell_id, cell).await
}

pub async fn fetch_cell<T: ConsoleStore>(
    db: &T,
    cell_id: Uuid,
) -> Result<Option<ConsoleCell>, SendableError> {
    db.fetch_console_cell(cell_id).await
}

pub async fn delete_cell<T: ConsoleStore>(db: &T, cell_id: Uuid) -> Result<bool, SendableError> {
    db.delete_console_cell(cell_id).await
}

/// the scope a cell runs against, built fresh from the session's stored bindings.
pub async fn session_context<T: ConsoleStore>(
    db: &T,
    session_id: Uuid,
) -> Result<ConsoleContext, SendableError> {
    let mut context = ConsoleContext::new();
    for binding in db.fetch_console_bindings(session_id).await? {
        context.bind(&binding.name, binding.value);
    }
    Ok(context)
}

/// run one cell.
///
/// `functions` is the published packaged-function catalog, threaded through so a console cell can
/// call one exactly as a workflow does — the console is the same language, so it must see the same
/// catalog.
pub async fn run_cell<
    T: ConsoleStore
        + RuntimeStore
        + DefinitionStore
        + FunctionStore
        + NotificationStore
        + ScheduleStore
        + WorkflowVmStore,
>(
    db: &T,
    cell_id: Uuid,
    providers: Vec<runinator_models::providers::ProviderMetadata>,
    functions: Vec<runinator_models::functions::FunctionCatalogEntry>,
) -> Result<CellOutcome, SendableError> {
    let Some(cell) = db.fetch_console_cell(cell_id).await? else {
        return Err(CONSOLE_CELL_NOT_FOUND.error(format!("cell {cell_id} not found")));
    };
    let options = runinator_rexrap::CompileOptions {
        enabled: true,
        providers,
        functions,
        ..runinator_rexrap::CompileOptions::default()
    };

    let classification = match runinator_console::classify(&cell.source, &options) {
        Ok(classification) => classification,
        Err(err) => return settle_failed(db, &cell, None, &err.to_string()).await,
    };
    let context = session_context(db, cell.session_id).await?;

    match classification.kind {
        CellKind::Expression | CellKind::Do => {
            let kind = match classification.kind {
                CellKind::Do => ConsoleCellKind::Do,
                _ => ConsoleCellKind::Expression,
            };
            let Some(fragment_kind) = classification.fragment_kind() else {
                return settle_failed(db, &cell, Some(kind), "cell has no evaluable form").await;
            };
            // evaluated through the same fragment evaluator `/rexrap/evaluate` uses. no second
            // evaluator: a console that computed `1 + 2` differently from an expression editor
            // would be a second language wearing the same syntax.
            match runinator_rexrap::evaluate_fragment(
                &cell.source,
                fragment_kind,
                &context.as_value(),
                &options,
            ) {
                Ok(value) => settle_succeeded(db, &cell, kind, value, None).await,
                Err(err) => settle_failed(db, &cell, Some(kind), &err.to_string()).await,
            }
        }
        CellKind::Workflow => run_scratch_workflow(db, &cell, &options, &context).await,
    }
}

/// attribute a settled scratch run back to the cell that started it.
///
/// called when a run reaches a terminal state. a console cell whose run finished but which still
/// reads `Running` is the failure mode this exists to prevent — the cell is the only place a
/// console user looks.
pub async fn settle_cell_for_run<T: ConsoleStore + RuntimeStore + WorkflowVmStore>(
    db: &T,
    workflow_run_id: Uuid,
) -> Result<Option<ConsoleCell>, SendableError> {
    let Some(cell) = db.fetch_console_cell_for_run(workflow_run_id).await? else {
        return Ok(None);
    };
    if cell.status.is_terminal() {
        return Ok(Some(cell));
    }
    let Some(run) = db.fetch_workflow_run(workflow_run_id).await? else {
        return Ok(Some(cell));
    };
    if !run.status.is_terminal() {
        return Ok(Some(cell));
    }

    let output = db
        .fetch_workflow_journal(workflow_run_id)
        .await?
        .into_iter()
        .rev()
        .find_map(|entry| match entry.entry {
            runinator_models::workflow_vm::WorkflowJournalEntry::Completed { value, .. } => {
                Some(value)
            }
            _ => None,
        })
        .unwrap_or(Value::Null);

    if run.status == runinator_models::workflows::WorkflowStatus::Succeeded {
        let outcome =
            settle_succeeded(db, &cell, ConsoleCellKind::Workflow, output, Some(run.id)).await?;
        return Ok(Some(outcome.cell));
    }
    let message = run
        .message
        .clone()
        .unwrap_or_else(|| format!("run finished {}", run.status.as_str()));
    let outcome = settle_failed_with_run(
        db,
        &cell,
        Some(ConsoleCellKind::Workflow),
        &message,
        Some(run.id),
    )
    .await?;
    Ok(Some(outcome.cell))
}

// compile the cell into a scratch workflow and start a run of it.
async fn run_scratch_workflow<
    T: ConsoleStore
        + RuntimeStore
        + DefinitionStore
        + FunctionStore
        + NotificationStore
        + ScheduleStore
        + WorkflowVmStore,
>(
    db: &T,
    cell: &ConsoleCell,
    options: &runinator_rexrap::CompileOptions,
    context: &ConsoleContext,
) -> Result<CellOutcome, SendableError> {
    let name = scratch_workflow_name(cell.session_id, cell.id);
    let source = runinator_console::workflow_source(&cell.source, &name);
    let mut definition = match runinator_rexrap::compile_str(&source, options) {
        Ok(definition) => definition,
        Err(err) => {
            return settle_failed(db, cell, Some(ConsoleCellKind::Workflow), &err.to_string())
                .await;
        }
    };
    // marked managed so it is filtered out of the workflow list exactly as a function adapter is:
    // one scratch workflow per cell run would otherwise bury the authored ones.
    stamp_managed(&mut definition);
    definition.name = name;
    if let Some(existing) = db.fetch_workflow_by_name(definition.name.clone()).await? {
        definition.id = existing.id;
    }
    if let Some(session) = db.fetch_console_session(cell.session_id).await? {
        definition.org_id = session.org_id;
    }

    let saved = match super::definitions::upsert_workflow(
        db,
        &definition,
        &RevisionAuthor {
            source: RevisionSource::Api,
            actor_id: None,
            actor_kind: CONSOLE_MANAGED_BY.to_string(),
            note: Some(format!("console cell {}", cell.id)),
        },
    )
    .await
    {
        Ok(saved) => saved,
        Err(err) => {
            return settle_failed(db, cell, Some(ConsoleCellKind::Workflow), &err.to_string())
                .await;
        }
    };
    let Some(workflow_id) = saved.id else {
        return settle_failed(
            db,
            cell,
            Some(ConsoleCellKind::Workflow),
            "scratch workflow was saved without an id",
        )
        .await;
    };

    let provenance = WorkflowRunProvenance {
        source_kind: Some(TriggerSourceKind::Console),
        actor_type: Some(TriggerActorType::User),
        actor_display_name: Some(format!("console cell {}", cell.position)),
        metadata: runinator_models::json!({
            "session_id": cell.session_id,
            "cell_id": cell.id,
        }),
        ..Default::default()
    };
    // the session's scope *is* the run's parameters, so `params.*` resolves inside the scratch
    // workflow exactly as it did in a pure cell — one meaning for a name, either way a cell runs.
    let run = super::runs::create_workflow_run(
        db,
        workflow_id,
        context.as_parameters(),
        false,
        Some(format!("console cell {}", cell.position)),
        provenance,
    )
    .await?;

    let cell = db
        .record_console_cell_outcome(
            cell.id,
            Some(ConsoleCellKind::Workflow),
            ConsoleCellStatus::Running,
            None,
            None,
            Some(run.id),
        )
        .await?
        .unwrap_or_else(|| cell.clone());
    Ok(CellOutcome {
        cell,
        run: Some(run),
    })
}

// record a success and bind the result into the session's scope.
async fn settle_succeeded<T: ConsoleStore>(
    db: &T,
    cell: &ConsoleCell,
    kind: ConsoleCellKind,
    value: Value,
    workflow_run_id: Option<Uuid>,
) -> Result<CellOutcome, SendableError> {
    let name = cell_binding_name(cell.label.as_deref(), cell.position);
    db.upsert_console_binding(cell.session_id, &name, Some(cell.id), &value)
        .await?;
    let cell = db
        .record_console_cell_outcome(
            cell.id,
            Some(kind),
            ConsoleCellStatus::Succeeded,
            Some(&value),
            None,
            workflow_run_id,
        )
        .await?
        .unwrap_or_else(|| cell.clone());
    Ok(CellOutcome { cell, run: None })
}

async fn settle_failed<T: ConsoleStore>(
    db: &T,
    cell: &ConsoleCell,
    kind: Option<ConsoleCellKind>,
    message: &str,
) -> Result<CellOutcome, SendableError> {
    settle_failed_with_run(db, cell, kind, message, None).await
}

// a failure does *not* bind: leaving the previous value under the name would make a later cell read
// a stale result while the cell that produced it is visibly red.
async fn settle_failed_with_run<T: ConsoleStore>(
    db: &T,
    cell: &ConsoleCell,
    kind: Option<ConsoleCellKind>,
    message: &str,
    workflow_run_id: Option<Uuid>,
) -> Result<CellOutcome, SendableError> {
    let name = cell_binding_name(cell.label.as_deref(), cell.position);
    let _ = db.delete_console_binding(cell.session_id, &name).await;
    let cell = db
        .record_console_cell_outcome(
            cell.id,
            kind,
            ConsoleCellStatus::Failed,
            None,
            Some(message),
            workflow_run_id,
        )
        .await?
        .unwrap_or_else(|| cell.clone());
    Ok(CellOutcome { cell, run: None })
}

/// true when a workflow is a console scratch workflow rather than an authored one.
pub fn is_console_workflow(definition: &runinator_models::workflows::WorkflowDefinition) -> bool {
    definition
        .definition
        .metadata
        .get("managed_by")
        .and_then(Value::as_str)
        == Some(CONSOLE_MANAGED_BY)
}

fn stamp_managed(definition: &mut runinator_models::workflows::WorkflowDefinition) {
    let mut metadata = match &definition.definition.metadata {
        Value::Object(object) => object.clone(),
        _ => Default::default(),
    };
    metadata.insert(
        "managed_by".into(),
        Value::String(CONSOLE_MANAGED_BY.to_string()),
    );
    definition.definition.metadata = Value::Object(metadata);
}

/// the bindings of one session, for the API.
pub async fn fetch_bindings<T: ConsoleStore>(
    db: &T,
    session_id: Uuid,
) -> Result<Vec<ConsoleBinding>, SendableError> {
    db.fetch_console_bindings(session_id).await
}

#[cfg(test)]
#[path = "console_tests.rs"]
mod tests;
