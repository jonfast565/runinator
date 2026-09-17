use std::time::Duration;

mod auth;
mod settings;

use chrono::{DateTime, Utc};
use reqwest::{Client, Response, Url};
use runinator_comm::{AgentDirectiveKind, AgentDirectiveRecord};
use runinator_models::json;
use runinator_models::pipelines::{Pipeline, PipelineBundle, PipelineRun, PipelineRunDetail};
use runinator_models::value::Value;
use runinator_models::{
    api_routes::{
        api_freeze_window, api_replica_heartbeat, api_replica_offline, api_replica_providers,
        api_scheduler_workflow_run_claim_release, api_scheduler_workflow_run_claim_renew,
        api_workflow, api_workflow_continuation, api_workflow_duplicate, api_workflow_effect,
        api_workflow_effect_output, api_workflow_revision, api_workflow_revision_restore,
        api_workflow_revisions, api_workflow_run, api_workflow_run_command,
        api_workflow_run_continuations, api_workflow_run_cursors, api_workflow_run_effects,
        api_workflow_run_journal, api_workflow_run_rename, api_workflow_run_replay,
        api_workflow_run_transitions, api_workflow_runs, api_workflow_trigger,
        api_workflow_trigger_backfill, api_workflow_trigger_runs, api_workflow_triggers,
        API_APPROVALS, API_ARTIFACTS_CONTENT, API_CREDENTIALS, API_DIAGNOSTIC_LOGS,
        API_EXECUTION_PROFILES, API_FREEZE_WINDOWS, API_FUNCTIONS, API_FUNCTIONS_CATALOG,
        API_FUNCTION_ARTIFACTS, API_FUNCTION_EXPORTS, API_IDEMPOTENCY_KEYS,
        API_IDEMPOTENCY_KEYS_CLAIM, API_IDEMPOTENCY_KEYS_COMPLETE, API_IDEMPOTENCY_KEYS_RELEASE,
        API_PACKS_IMPORT, API_PROVIDERS, API_REPLICAS, API_SCHEDULER_WORKFLOW_RUNS_CLAIM,
        API_SUPERVISOR_STATUS, API_WORKFLOWS, API_WORKFLOWS_EXPORT, API_WORKFLOWS_SIMULATE,
        API_WORKFLOWS_VALIDATE, API_WORKFLOW_EFFECTS, API_WORKFLOW_FILES, API_WORKFLOW_RUNS,
        API_WORKFLOW_TRIGGERS_DUE,
    },
    auth::{
        AgentEnrollmentToken, AgentMachineEnrollment, CreateAgentEnrollmentTokenRequest,
        CreateAgentEnrollmentTokenResponse, EnrollAgentRequest, EnrollAgentResponse,
    },
    billing::ScaleOrgNodesRequest,
    bundles::{Bundle, PackImportResult, ProviderBundle, SettingsBundle},
    console::{ConsoleCell, ConsoleSession, ConsoleSessionDetail, NewConsoleCell},
    diagnostics::{RuntimeLogBatch, RuntimeLogPage, RuntimeLogQuery},
    execution_profiles::{
        ExecutionProfile, ExecutionProfileAgentStatusRequest, ExecutionProfileCollectionStatus,
        ExecutionProfileOperation, ExecutionProfileOperationClaimRequest,
        ExecutionProfileOperationCompleteRequest, ExecutionProfilePublishRequest,
        ExecutionProfilePutRequest, ExecutionProfileRevision, ExecutionProfileStatusRequest,
    },
    functions::{
        FunctionAlias, FunctionArtifact, FunctionCatalogEntry, FunctionInvocationTarget,
        FunctionPackage, FunctionPackageDetail, FunctionVersion, NewFunctionVersion,
        ARTIFACT_MEDIA_TYPE,
    },
    orchestration::{
        AdapterDefinition, AdapterKindCatalogEntry, AdapterRevision, IdempotencyClaim,
        IdempotencyClaimRequest, IdempotencyCompleteRequest, IdempotencyReleaseRequest,
        OrchestrationBinding, OrchestrationCommand, OrchestrationCorrelationAlias,
        OrchestrationEpoch, OrchestrationEventReduction, OrchestrationEvidence,
        ACTION_IDEMPOTENCY_SCOPE,
    },
    providers::ProviderMetadata,
    provisioning::{NodeBackendsResponse, ProvisionedGroup, ScaleNodesRequest, StopNodeRequest},
    replicas::{
        ReplicaHeartbeatRequest, ReplicaKind, ReplicaListResponse, ReplicaOfflineRequest,
        ReplicaProviderRegistration, ReplicaProviderRegistrationRequest, ReplicaRecord,
        ReplicaRegistrationRequest, ReplicaStatus,
    },
    revisions::{PipelineRevision, WorkflowRevision},
    runs::ProviderTerminalControl,
    schedules::{BackfillRequest, BackfillResponse, FreezeWindow, NewFreezeWindow},
    telemetry::ReplicaSampleSeries,
    web::TaskResponse,
    workflow_vm::{
        WorkflowContinuation, WorkflowEffect, WorkflowEffectOutputEvent, WorkflowEffectStatus,
        WorkflowJournalRecord, WorkflowVmCursor,
    },
    workflows::{
        WorkflowBundle, WorkflowDefinition, WorkflowRun, WorkflowSimulateRequest, WorkflowStatus,
        WorkflowTrigger,
    },
    workspaces::WorkspaceLease,
};
use serde::de::DeserializeOwned;
use tower::{service_fn, ServiceExt};
use tower_resilience_circuitbreaker::{CircuitBreakerError, CircuitBreakerLayer, FnClassifier};
use uuid::Uuid;

use crate::{
    error::{ApiError, Result},
    locator::ServiceLocator,
    types::{ArtifactContentResponse, IngressResponse, PipelineIngressRequest},
};

/// Default cap on a single request's total wall-clock time. Bounds a hung or slow web service so a
/// caller (worker/ctl/engine) fails fast and retries rather than parking a task indefinitely.
/// Override with `RUNINATOR_API_TIMEOUT_SECONDS`.
const DEFAULT_REQUEST_TIMEOUT_SECONDS: u64 = 60;

/// Default cap on establishing the TCP/TLS connection, separate from the overall request timeout so a
/// dead host is detected quickly. Override with `RUNINATOR_API_CONNECT_TIMEOUT_SECONDS`.
const DEFAULT_CONNECT_TIMEOUT_SECONDS: u64 = 10;
const DEFAULT_CIRCUIT_FAILURE_THRESHOLD: usize = 5;
const DEFAULT_CIRCUIT_COOLDOWN_SECONDS: u64 = 30;

type HttpResult = std::result::Result<Response, reqwest::Error>;
type HttpClassifier = fn(&HttpResult) -> bool;
type HttpCircuitLayer = CircuitBreakerLayer<FnClassifier<HttpClassifier>>;

fn outbound_failure(result: &HttpResult) -> bool {
    match result {
        Ok(response) => {
            response.status() == reqwest::StatusCode::REQUEST_TIMEOUT
                || response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS
                || response.status().is_server_error()
        }
        Err(_) => true,
    }
}

fn env_bool(key: &str, default: bool) -> bool {
    std::env::var(key)
        .ok()
        .and_then(|raw| raw.trim().parse().ok())
        .unwrap_or(default)
}

fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|raw| raw.trim().parse().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

fn env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|raw| raw.trim().parse().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

fn env_duration(key: &str, default_seconds: u64) -> Duration {
    let seconds = std::env::var(key)
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default_seconds);
    Duration::from_secs(seconds)
}

/// A `reqwest::ClientBuilder` preconfigured with the request/connect timeouts every client shares.
fn timed_client_builder() -> reqwest::ClientBuilder {
    Client::builder()
        .timeout(env_duration(
            "RUNINATOR_API_TIMEOUT_SECONDS",
            DEFAULT_REQUEST_TIMEOUT_SECONDS,
        ))
        .connect_timeout(env_duration(
            "RUNINATOR_API_CONNECT_TIMEOUT_SECONDS",
            DEFAULT_CONNECT_TIMEOUT_SECONDS,
        ))
}

#[cfg(test)]
#[path = "workspace_seal_tests.rs"]
mod workspace_seal_tests;

#[cfg(test)]
mod resilience_tests {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
        thread,
    };

    use super::*;

    fn status_server(statuses: Vec<u16>) -> (String, Arc<AtomicUsize>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let server_calls = calls.clone();
        let task = thread::spawn(move || {
            for status in statuses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0_u8; 1024];
                let _ = stream.read(&mut request);
                server_calls.fetch_add(1, Ordering::SeqCst);
                let reason = if status == 200 { "OK" } else { "Test Failure" };
                let body = if status == 200 { "{}" } else { "failure" };
                write!(
                    stream,
                    "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
        });
        (format!("http://{address}"), calls, task)
    }

    #[test]
    fn transient_upstream_failures_open_without_an_extra_network_call_and_a_probe_recovers() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let (base_url, calls, server) = status_server(vec![500, 500, 200]);
            let client = AsyncApiClient {
                client: Client::new(),
                circuit: ApiCircuit::new(true, 2, Duration::from_millis(1)),
                locator: crate::locator::StaticLocator::new(base_url.clone()),
            };

            for _ in 0..2 {
                let response = client
                    .send(client.client.get(format!("{base_url}/failing")))
                    .await
                    .unwrap();
                assert_eq!(
                    response.status(),
                    reqwest::StatusCode::INTERNAL_SERVER_ERROR
                );
            }
            let error = client
                .send(client.client.get(format!("{base_url}/skipped")))
                .await
                .unwrap_err();
            assert!(matches!(
                error,
                ApiError::CircuitOpen {
                    retry_after_seconds: 1,
                    ..
                }
            ));
            assert_eq!(calls.load(Ordering::SeqCst), 2);

            tokio::time::sleep(Duration::from_millis(5)).await;
            let response = client
                .send(client.client.get(format!("{base_url}/probe")))
                .await
                .unwrap();
            assert_eq!(response.status(), reqwest::StatusCode::OK);
            assert_eq!(calls.load(Ordering::SeqCst), 3);
            server.join().unwrap();
        });
    }

    #[test]
    fn ordinary_upstream_4xx_responses_do_not_open_the_api_circuit() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let (base_url, calls, server) = status_server(vec![400, 400, 400]);
            let client = AsyncApiClient {
                client: Client::new(),
                circuit: ApiCircuit::new(true, 2, Duration::from_secs(1)),
                locator: crate::locator::StaticLocator::new(base_url.clone()),
            };
            for _ in 0..3 {
                let response = client
                    .send(client.client.get(format!("{base_url}/client-error")))
                    .await
                    .unwrap();
                assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
            }
            assert_eq!(calls.load(Ordering::SeqCst), 3);
            server.join().unwrap();
        });
    }
}

mod orchestration_list_query;
pub use orchestration_list_query::OrchestrationListQuery;

mod api_circuit;
use api_circuit::ApiCircuit;

mod async_api_client;
pub use async_api_client::AsyncApiClient;
