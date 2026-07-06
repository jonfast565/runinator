use std::{collections::HashMap, fs, path::PathBuf, sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use runinator_models::providers::ActionMetadata;
use runinator_models::runs::{ProviderExecutionRequest, RunStatus, TaskExecutionResult};
use runinator_models::value::Value;
use runinator_models::workflows::WorkflowAction;
use runinator_plugin::cancel::CancellationToken;
use runinator_plugin::plugin::Plugin;
use runinator_plugin::provider::{Provider, ProviderEventSink};
use runinator_utilities::app_data;
use tokio::time;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::provider_repository::{ProviderFactory, resolve_provider};

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
    providers: &ProviderFactory,
    libraries: Arc<HashMap<String, Plugin>>,
    action: WorkflowAction,
    workflow_node_run_id: Uuid,
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

    match resolve_provider(providers, &libraries, &action) {
        Ok(provider) => {
            let action_metadata = match provider_action_metadata(provider.as_ref(), &action) {
                Ok(metadata) => metadata,
                Err(message) => {
                    error!(provider = %action.provider, function = %action.function, "{}", message);
                    return failed_outcome(started_at, message);
                }
            };
            if let Err(message) =
                validate_runtime_parameters(&action_metadata, &action, &request.parameters)
            {
                error!(provider = %action.provider, function = %action.function, "{}", message);
                return failed_outcome(started_at, message);
            }
            let provider_token = token.clone();
            // the provider runs on a blocking thread, which does not inherit the ambient tracing
            // span automatically; enter it explicitly so provider-side log lines keep trace_id/run_id.
            let exec_span = tracing::Span::current();
            let mut handle = tokio::task::spawn_blocking(move || {
                let _guard = exec_span.enter();
                provider.execute_service(request, sink, provider_token)
            });

            tokio::select! {
                join_result = &mut handle => match join_result {
                    Ok(Ok(execution_result)) => {
                        let finished_at = Utc::now();
                        if let Err(message) = validate_execution_result(&action_metadata, &action, &execution_result) {
                            error!(provider = %action.provider, function = %action.function, "{}", message);
                            return failed_outcome(started_at, message);
                        }
                        info!(
                            provider = %action.provider,
                            function = %action.function,
                            duration_ms = (finished_at - started_at).num_milliseconds(),
                            "action completed successfully"
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
                            provider = %action.provider,
                            function = %action.function,
                            error_code = runinator_models::errors::error_code_or_unknown(err.as_ref()),
                            "provider execution error: {}",
                            err
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
                        error!(provider = %action.provider, function = %action.function, "task panicked: {:?}", err);
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
                        provider = %action.provider,
                        function = %action.function,
                        timeout_secs = timeout,
                        "action exceeded timeout"
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
                    warn!(provider = %action.provider, function = %action.function, "action received cancellation");
                    canceled_outcome(started_at)
                }
            }
        }
        Err(err) => {
            error!(
                provider = %action.provider,
                function = %action.function,
                error_code = runinator_models::errors::error_code_or_unknown(err.as_ref()),
                "failed to resolve provider: {}",
                err
            );
            failed_outcome(started_at, err.to_string())
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

fn failed_outcome(started_at: DateTime<Utc>, message: String) -> ExecutionOutcome {
    ExecutionOutcome {
        execution_result: None,
        status: RunStatus::Failed,
        task_result: ExecutionTaskResult {
            success: false,
            started_at,
            finished_at: Utc::now(),
            message: Some(message),
        },
    }
}

fn provider_action_metadata(
    provider: &dyn Provider,
    action: &WorkflowAction,
) -> Result<ActionMetadata, String> {
    let metadata = provider.metadata();
    let Some(action_metadata) = metadata
        .actions
        .iter()
        .find(|candidate| candidate.function_name == action.function)
    else {
        return Err(format!(
            "Provider '{}' does not advertise action '{}'",
            action.provider, action.function
        ));
    };
    Ok(action_metadata.clone())
}

fn validate_runtime_parameters(
    action_metadata: &ActionMetadata,
    action: &WorkflowAction,
    parameters: &Value,
) -> Result<(), String> {
    let expected = action_metadata.parameters_type();
    expected.validate_value(parameters).map_err(|violation| {
        violation.message_with_label(&format!(
            "resolved action configuration '{}.{}'",
            action.provider, action.function
        ))
    })
}

pub(crate) fn validate_execution_result(
    action_metadata: &ActionMetadata,
    action: &WorkflowAction,
    result: &TaskExecutionResult,
) -> Result<(), String> {
    let Some(output) = result.output_json.as_ref() else {
        return Ok(());
    };
    let expected = action_metadata.results_type();
    expected.validate_value(output).map_err(|violation| {
        violation.message_with_label(&format!(
            "provider output '{}.{}'",
            action.provider, action.function
        ))
    })
}

fn build_provider_request(
    action: &WorkflowAction,
    workflow_node_run_id: Uuid,
    parameters: Value,
) -> ProviderExecutionRequest {
    let base_dir = run_work_dir(Some(workflow_node_run_id));
    let artifact_dir = base_dir.join("artifacts");
    if let Err(err) = fs::create_dir_all(&artifact_dir) {
        warn!(
            node_id = %workflow_node_run_id,
            artifact_dir = %artifact_dir.display(),
            "failed to create artifact directory: {}",
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

fn run_work_dir(run_id: Option<Uuid>) -> PathBuf {
    let suffix = run_id
        .map(|id| id.to_string())
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    app_data::app_data_path("worker/runs")
        .unwrap_or_else(|_| std::env::temp_dir().join("runinator-worker"))
        .join(suffix)
}
