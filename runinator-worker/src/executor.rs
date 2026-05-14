use std::{collections::HashMap, fs, path::PathBuf, sync::Arc, time::Duration};

use chrono::Utc;
use log::{error, info, warn};
use runinator_comm::TaskResult;
use runinator_models::runs::{ProviderExecutionRequest, RunStatus, TaskExecutionResult};
use runinator_models::workflows::WorkflowAction;
use runinator_plugin::plugin::Plugin;
use runinator_plugin::provider::ProviderEventSink;
use serde_json::Value;
use tokio::time;
use uuid::Uuid;

use crate::provider_repository::resolve_provider;

pub struct ExecutionOutcome {
    pub task_result: TaskResult,
    pub execution_result: Option<TaskExecutionResult>,
    pub status: RunStatus,
}

pub async fn execute_task(
    libraries: Arc<HashMap<String, Plugin>>,
    command_id: Uuid,
    action: WorkflowAction,
    workflow_node_run_id: i64,
    parameters: Value,
    sink: Option<Arc<dyn ProviderEventSink>>,
) -> ExecutionOutcome {
    let started_at = Utc::now();
    let timeout = action.timeout_seconds.max(1) as u64;
    let request = build_provider_request(&action, workflow_node_run_id, parameters);

    match resolve_provider(&libraries, &action) {
        Ok(provider) => {
            let handle =
                tokio::task::spawn_blocking(move || provider.execute_service(request, sink));

            match time::timeout(Duration::from_secs(timeout), handle).await {
                Ok(join_result) => match join_result {
                    Ok(Ok(execution_result)) => {
                        let finished_at = Utc::now();
                        info!(
                            "Action {}.{} completed successfully",
                            action.provider, action.function
                        );
                        let message = execution_result.message.clone();
                        ExecutionOutcome {
                            execution_result: Some(execution_result),
                            status: RunStatus::Succeeded,
                            task_result: TaskResult {
                                command_id,
                                success: true,
                                started_at,
                                finished_at,
                                message,
                            },
                        }
                    }
                    Ok(Err(err)) => {
                        error!(
                            "Provider execution error for action {}.{}: {}",
                            action.provider, action.function, err
                        );
                        ExecutionOutcome {
                            execution_result: None,
                            status: RunStatus::Failed,
                            task_result: TaskResult {
                                command_id,
                                success: false,
                                started_at,
                                finished_at: Utc::now(),
                                message: Some(err.to_string()),
                            },
                        }
                    }
                    Err(err) => {
                        error!("Task panicked: {:?}", err);
                        ExecutionOutcome {
                            execution_result: None,
                            status: RunStatus::Failed,
                            task_result: TaskResult {
                                command_id,
                                success: false,
                                started_at,
                                finished_at: Utc::now(),
                                message: Some("Task panicked during execution".into()),
                            },
                        }
                    }
                },
                Err(_) => {
                    warn!(
                        "Action {}.{} exceeded timeout of {} seconds",
                        action.provider, action.function, timeout
                    );
                    ExecutionOutcome {
                        execution_result: None,
                        status: RunStatus::TimedOut,
                        task_result: TaskResult {
                            command_id,
                            success: false,
                            started_at,
                            finished_at: Utc::now(),
                            message: Some(format!("Task timed out after {} seconds", timeout)),
                        },
                    }
                }
            }
        }
        Err(err) => {
            error!(
                "Failed to resolve provider for action {}.{}: {}",
                action.provider, action.function, err
            );
            ExecutionOutcome {
                execution_result: None,
                status: RunStatus::Failed,
                task_result: TaskResult {
                    command_id,
                    success: false,
                    started_at,
                    finished_at: Utc::now(),
                    message: Some(err.to_string()),
                },
            }
        }
    }
}

fn build_provider_request(
    action: &WorkflowAction,
    workflow_node_run_id: i64,
    parameters: Value,
) -> ProviderExecutionRequest {
    let base_dir = run_work_dir(Some(workflow_node_run_id));
    let artifact_dir = base_dir.join("artifacts");
    if let Err(err) = fs::create_dir_all(&artifact_dir) {
        warn!(
            "Failed to create artifact directory {}: {}",
            artifact_dir.display(),
            err
        );
    }
    ProviderExecutionRequest {
        run_id: Some(workflow_node_run_id),
        action_name: action.provider.clone(),
        action_function: action.function.clone(),
        parameters,
        timeout_secs: action.timeout_seconds,
        artifact_dir: artifact_dir.to_string_lossy().into_owned(),
        events_jsonl_path: base_dir.join("events.jsonl").to_string_lossy().into_owned(),
    }
}

fn run_work_dir(run_id: Option<i64>) -> PathBuf {
    let suffix = run_id
        .map(|id| id.to_string())
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    std::env::temp_dir().join("runinator-worker").join(suffix)
}
