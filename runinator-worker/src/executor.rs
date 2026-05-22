use std::{collections::HashMap, fs, path::PathBuf, sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use log::{error, info, warn};
use runinator_models::runs::{ProviderExecutionRequest, RunStatus, TaskExecutionResult};
use runinator_models::workflows::WorkflowAction;
use runinator_plugin::cancel::CancellationToken;
use runinator_plugin::plugin::Plugin;
use runinator_plugin::provider::ProviderEventSink;
use runinator_utilities::app_data;
use serde_json::Value;
use tokio::time;
use uuid::Uuid;

use crate::provider_repository::resolve_provider;

pub struct ExecutionOutcome {
    pub task_result: ExecutionTaskResult,
    pub execution_result: Option<TaskExecutionResult>,
    pub status: RunStatus,
}

pub struct ExecutionTaskResult {
    pub success: bool,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub message: Option<String>,
}

impl ExecutionTaskResult {
    pub fn duration_ms(&self) -> i64 {
        (self.finished_at - self.started_at).num_milliseconds()
    }
}

pub async fn execute_task(
    libraries: Arc<HashMap<String, Plugin>>,
    action: WorkflowAction,
    workflow_node_run_id: i64,
    parameters: Value,
    sink: Option<Arc<dyn ProviderEventSink>>,
    token: CancellationToken,
) -> ExecutionOutcome {
    let started_at = Utc::now();
    let timeout = action.timeout_seconds.max(1) as u64;
    let request = build_provider_request(&action, workflow_node_run_id, parameters);

    if token.is_cancelled() {
        return canceled_outcome(started_at);
    }

    match resolve_provider(&libraries, &action) {
        Ok(provider) => {
            let provider_token = token.clone();
            let mut handle = tokio::task::spawn_blocking(move || {
                provider.execute_service(request, sink, provider_token)
            });

            tokio::select! {
                join_result = &mut handle => match join_result {
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
                            task_result: ExecutionTaskResult {
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
                            task_result: ExecutionTaskResult {
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
                            task_result: ExecutionTaskResult {
                                success: false,
                                started_at,
                                finished_at: Utc::now(),
                                message: Some("Task panicked during execution".into()),
                            },
                        }
                    }
                },
                _ = time::sleep(Duration::from_secs(timeout)) => {
                    token.cancel();
                    warn!(
                        "Action {}.{} exceeded timeout of {} seconds",
                        action.provider, action.function, timeout
                    );
                    ExecutionOutcome {
                        execution_result: None,
                        status: RunStatus::TimedOut,
                        task_result: ExecutionTaskResult {
                            success: false,
                            started_at,
                            finished_at: Utc::now(),
                            message: Some(format!("Task timed out after {} seconds", timeout)),
                        },
                    }
                },
                _ = wait_for_cancel(token.clone()) => {
                    warn!(
                        "Action {}.{} received cancellation",
                        action.provider, action.function
                    );
                    canceled_outcome(started_at)
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
                task_result: ExecutionTaskResult {
                    success: false,
                    started_at,
                    finished_at: Utc::now(),
                    message: Some(err.to_string()),
                },
            }
        }
    }
}

async fn wait_for_cancel(token: CancellationToken) {
    while !token.is_cancelled() {
        time::sleep(Duration::from_millis(100)).await;
    }
}

fn canceled_outcome(started_at: DateTime<Utc>) -> ExecutionOutcome {
    ExecutionOutcome {
        execution_result: None,
        status: RunStatus::Canceled,
        task_result: ExecutionTaskResult {
            success: false,
            started_at,
            finished_at: Utc::now(),
            message: Some("Task canceled".into()),
        },
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
    app_data::app_data_path("worker/runs")
        .unwrap_or_else(|_| std::env::temp_dir().join("runinator-worker"))
        .join(suffix)
}
