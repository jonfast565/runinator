use std::{collections::HashMap, fs, path::PathBuf, sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use runinator_models::execution_profiles::MaterializedExecutionProfile;
use runinator_models::providers::{
    ActionMetadata, CredentialInjection, validate_action_authentication,
};
use runinator_models::runs::{
    MaterializedCredentialInjections, ProviderExecutionRequest, RunStatus, TaskExecutionResult,
};
use runinator_models::value::Value;
use runinator_models::workflows::WorkflowAction;
use runinator_platform::app_data;
use runinator_plugin::cancel::CancellationToken;
use runinator_plugin::plugin::Plugin;
use runinator_plugin::provider::{Provider, ProviderEventSink};
use tokio::time;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::provider_repository::{ProviderFactory, resolve_provider};

pub(crate) struct ExecutionProfileMetadata {
    pub support: runinator_models::providers::ExecutionProfileSupport,
    pub credential_scopes: Vec<String>,
}

pub(crate) fn execution_profile_metadata(
    providers: &ProviderFactory,
    libraries: &HashMap<String, Plugin>,
    action: &WorkflowAction,
) -> Result<ExecutionProfileMetadata, String> {
    let provider =
        resolve_provider(providers, libraries, action).map_err(|error| error.to_string())?;
    let metadata = provider.metadata();
    let action_metadata = metadata
        .actions
        .iter()
        .find(|candidate| candidate.function_name == action.function)
        .ok_or_else(|| {
            format!(
                "provider action '{}.{}' was not found",
                action.provider, action.function
            )
        })?;
    Ok(ExecutionProfileMetadata {
        support: metadata.metadata.execution_profile,
        credential_scopes: action_metadata
            .credential_scopes
            .clone()
            .unwrap_or(metadata.metadata.credential_scopes),
    })
}

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
pub(crate) struct TaskExecution {
    pub libraries: Arc<HashMap<String, Plugin>>,
    pub action: WorkflowAction,
    pub execution_id: Uuid,
    pub parameters: Value,
    pub idempotency_key: Option<String>,
    pub execution_profile: Option<MaterializedExecutionProfile>,
    pub sink: Option<Arc<dyn ProviderEventSink>>,
    pub token: CancellationToken,
}

pub(crate) async fn execute_task(
    providers: &ProviderFactory,
    task: TaskExecution,
) -> ExecutionOutcome {
    let TaskExecution {
        libraries,
        action,
        execution_id,
        parameters,
        idempotency_key,
        execution_profile,
        sink,
        token,
    } = task;
    let started_at = Utc::now();
    let timeout = action.timeout_seconds.max(1) as u64;
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
                validate_runtime_parameters(&action_metadata, &action, &parameters)
            {
                error!(provider = %action.provider, function = %action.function, "{}", message);
                return failed_outcome(started_at, message);
            }
            if let Err(message) = validate_action_authentication(
                &action_metadata,
                &parameters,
                execution_profile.is_some(),
            ) {
                error!(provider = %action.provider, function = %action.function, "{}", message);
                return failed_outcome(started_at, message);
            }
            let (parameters, credential_injections) =
                match materialize_credential_injections(&action_metadata, parameters) {
                    Ok(materialized) => materialized,
                    Err(message) => return failed_outcome(started_at, message),
                };
            let request = build_provider_request(
                &action,
                execution_id,
                parameters,
                idempotency_key,
                execution_profile,
                credential_injections,
            );
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
    let mut effective = action_metadata.clone();
    if effective.authentication.is_some() {
        let secret_parameters = effective
            .authentication
            .iter()
            .flat_map(|authentication| &authentication.alternatives)
            .filter_map(|alternative| match alternative {
                runinator_models::providers::ActionAuthenticationAlternative::Secrets {
                    parameters,
                } => Some(parameters),
                _ => None,
            })
            .flatten()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        for parameter in &mut effective.parameters {
            if secret_parameters.contains(&parameter.name) {
                parameter.required = false;
            }
        }
    }
    let expected = effective.parameters_type();
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
    // an action that declares no results is unconstrained, not constrained to return nothing.
    // `results_type()` builds a *closed* structure, so an empty result list would reject every
    // field a provider returned — and providers whose output is arbitrary by nature (`std.run` and
    // `std.exec` return whatever their program returned) cannot describe it in advance.
    if action_metadata.results.is_empty() {
        return Ok(());
    }
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
    execution_id: Uuid,
    parameters: Value,
    idempotency_key: Option<String>,
    execution_profile: Option<MaterializedExecutionProfile>,
    credential_injections: MaterializedCredentialInjections,
) -> ProviderExecutionRequest {
    let base_dir = run_work_dir(Some(execution_id));
    let artifact_dir = base_dir.join("artifacts");
    if let Err(err) = fs::create_dir_all(&artifact_dir) {
        warn!(
            execution_id = %execution_id,
            artifact_dir = %artifact_dir.display(),
            "failed to create artifact directory: {}",
            err
        );
    }
    ProviderExecutionRequest {
        run_id: Some(execution_id),
        action_name: action.provider.clone(),
        action_function: action.function.clone(),
        parameters,
        timeout_secs: action.timeout_seconds,
        artifact_dir: artifact_dir.to_string_lossy().into_owned(),
        events_jsonl_path: base_dir.join("events.jsonl").to_string_lossy().into_owned(),
        idempotency_key,
        workspace_path: resolved_workspace_path(action.workspace_affinity.as_ref()),
        execution_profile,
        credential_injections,
    }
}

fn materialize_credential_injections(
    metadata: &ActionMetadata,
    mut parameters: Value,
) -> Result<(Value, MaterializedCredentialInjections), String> {
    let mut materialized = MaterializedCredentialInjections::default();
    let Some(object) = parameters.as_object_mut() else {
        return Ok((parameters, materialized));
    };
    for parameter in &metadata.parameters {
        if parameter.credential_injections.is_empty() {
            continue;
        }
        let Some(secret) = object
            .get(&parameter.name)
            .and_then(Value::as_str)
            .map(str::to_owned)
        else {
            continue;
        };
        object.remove(&parameter.name);
        for injection in &parameter.credential_injections {
            match injection {
                CredentialInjection::Parameter { name, template } => {
                    object.insert(
                        name.clone(),
                        Value::String(render_secret(template, &secret)),
                    );
                }
                CredentialInjection::Environment { name, template } => {
                    materialized
                        .environment
                        .insert(name.clone(), render_secret(template, &secret));
                }
                CredentialInjection::Arguments { values } => {
                    materialized
                        .arguments
                        .extend(values.iter().map(|value| render_secret(value, &secret)));
                }
                CredentialInjection::Header { name, template } => {
                    materialized
                        .headers
                        .insert(name.clone(), render_secret(template, &secret));
                }
            }
        }
    }
    Ok((parameters, materialized))
}

fn render_secret(template: &str, secret: &str) -> String {
    template.replace("${secret}", secret)
}

fn resolved_workspace_path(affinity: Option<&Value>) -> Option<String> {
    affinity
        .and_then(|affinity| affinity.get("resolved_path"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn run_work_dir(run_id: Option<Uuid>) -> PathBuf {
    let suffix = run_id
        .map(|id| id.to_string())
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    app_data::app_data_path("worker/runs")
        .unwrap_or_else(|_| std::env::temp_dir().join("runinator-worker"))
        .join(suffix)
}

#[cfg(test)]
mod workspace_request_tests {
    use super::resolved_workspace_path;
    use runinator_models::json;

    #[test]
    fn only_worker_resolved_affinity_path_is_exposed() {
        let affinity = json!({
            "local_key": "bindings/example/source/1",
            "resolved_path": "/worker-root/bindings/example/source/1"
        });
        assert_eq!(
            resolved_workspace_path(Some(&affinity)).as_deref(),
            Some("/worker-root/bindings/example/source/1")
        );
        assert_eq!(
            resolved_workspace_path(Some(&json!({ "local_key": "x" }))),
            None
        );
        assert_eq!(resolved_workspace_path(None), None);
    }
}

#[cfg(test)]
#[path = "executor_tests.rs"]
mod executor_tests;
