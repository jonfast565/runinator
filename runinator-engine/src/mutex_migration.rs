//! reconcile live json-backed mutex state into the normalized mutex tables at engine startup.

use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, Utc};
use runinator_database::interfaces::DatabaseImpl;
use runinator_database::workflow_mutex::WorkflowMutexClaim;
use runinator_models::{
    errors::SendableError,
    value::Value,
    workflows::{WorkflowRun, WorkflowStatus},
};
use uuid::Uuid;

const LEGACY_RECORD_TYPE: &str = "workflow_mutex";

pub(super) async fn reconcile_legacy_mutexes<T: DatabaseImpl>(db: &T) -> Result<(), SendableError> {
    let records = db
        .fetch_automation_records(LEGACY_RECORD_TYPE.into(), None, None)
        .await?;
    let mut active_by_name: BTreeMap<String, Vec<(Uuid, Value)>> = BTreeMap::new();
    let mut run_cache: HashMap<Uuid, Option<WorkflowRun>> = HashMap::new();
    for record in records {
        if record.get("released_at").is_some() {
            continue;
        }
        let (Some(name), Some(run_id)) = (
            record.get("name").and_then(Value::as_str),
            record
                .get("held_by_run_id")
                .and_then(Value::as_str)
                .and_then(|raw| raw.parse::<Uuid>().ok()),
        ) else {
            continue;
        };
        let run = match run_cache.get(&run_id) {
            Some(run) => run.clone(),
            None => {
                let run = db.fetch_workflow_run(run_id).await?;
                run_cache.insert(run_id, run.clone());
                run
            }
        };
        if run.is_some_and(|run| !run.status.is_terminal()) {
            active_by_name
                .entry(name.to_string())
                .or_default()
                .push((run_id, record));
        }
    }

    for (name, holders) in &active_by_name {
        let mut runs = holders
            .iter()
            .map(|(run_id, _)| *run_id)
            .collect::<Vec<_>>();
        runs.sort_unstable();
        runs.dedup();
        if runs.len() > 1 {
            return Err(crate::errors::MUTEX_MIGRATION_CONFLICT.error(format!(
                "mutex '{name}' has active legacy holders {}",
                runs.iter()
                    .map(Uuid::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }
    }

    // restore holders before waiters so a waiter can never be admitted ahead of work that was
    // already inside its critical section when the normalized tables were introduced.
    for (name, holders) in active_by_name {
        let (run_id, record) = holders
            .into_iter()
            .min_by_key(|(_, record)| record_unix(record, "created_at"))
            .expect("active holder group is non-empty");
        let node_run = db
            .fetch_workflow_node_runs(run_id)
            .await?
            .into_iter()
            .filter(|node_run| {
                node_run.transition_reason.as_deref() == Some("mutex_acquired")
                    && mutex_name(&node_run.parameters, &node_run.node_id) == name
            })
            .max_by_key(|node_run| node_run.created_at)
            .ok_or_else(|| {
                crate::errors::MUTEX_MIGRATION_CONFLICT.error(format!(
                    "active legacy mutex '{name}' holder {run_id} has no acquisition node run"
                ))
            })?;
        let cursor_id = node_run.cursor_id.ok_or_else(|| {
            crate::errors::MUTEX_MIGRATION_CONFLICT.error(format!(
                "active legacy mutex '{name}' holder {run_id} has no cursor attribution"
            ))
        })?;
        let result = db
            .claim_workflow_mutex(
                WorkflowMutexClaim {
                    name: name.clone(),
                    workflow_run_id: run_id,
                    workflow_node_run_id: node_run.id,
                    cursor_id,
                    node_id: node_run.node_id,
                    hold_deadline_unix: record.get("lease_deadline").and_then(Value::as_i64),
                    enqueued_at_unix: record_unix(&record, "created_at"),
                },
                Utc::now().timestamp(),
            )
            .await?;
        if !result.acquired {
            return Err(crate::errors::MUTEX_MIGRATION_CONFLICT.error(format!(
                "normalized mutex '{name}' conflicts with active legacy holder {run_id}"
            )));
        }
        if result.holder_overdue {
            tracing::warn!(
                mutex = %name,
                holder_run_id = %run_id,
                "preserved overdue active legacy mutex holder during migration"
            );
        }
    }

    for node_run in db
        .fetch_workflow_node_runs_by_status(WorkflowStatus::Waiting)
        .await?
        .into_iter()
        .filter(|node_run| node_run.transition_reason.as_deref() == Some("mutex_waiting"))
    {
        let Some(cursor_id) = node_run.cursor_id else {
            continue;
        };
        let Some(run) = db.fetch_workflow_run(node_run.workflow_run_id).await? else {
            continue;
        };
        if run.status.is_terminal() {
            continue;
        }
        let name = node_run
            .state
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| mutex_name(&node_run.parameters, &node_run.node_id));
        db.claim_workflow_mutex(
            WorkflowMutexClaim {
                name,
                workflow_run_id: node_run.workflow_run_id,
                workflow_node_run_id: node_run.id,
                cursor_id,
                node_id: node_run.node_id,
                hold_deadline_unix: None,
                enqueued_at_unix: node_run.created_at.timestamp(),
            },
            Utc::now().timestamp(),
        )
        .await?;
    }
    Ok(())
}

fn mutex_name(parameters: &Value, fallback: &str) -> String {
    parameters
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(fallback)
        .to_string()
}

fn record_unix(record: &Value, field: &str) -> i64 {
    record
        .get(field)
        .and_then(Value::as_i64)
        .or_else(|| {
            record
                .get(field)
                .and_then(Value::as_str)
                .and_then(|raw| DateTime::parse_from_rfc3339(raw).ok())
                .map(|value| value.timestamp())
        })
        .unwrap_or_else(|| Utc::now().timestamp())
}
