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
        api_artifact_download, api_freeze_window, api_replica_heartbeat, api_replica_offline,
        api_replica_providers, api_run, api_run_artifacts, api_run_chunks,
        api_scheduler_workflow_run_claim_release, api_scheduler_workflow_run_claim_renew,
        api_workflow, api_workflow_continuation, api_workflow_duplicate, api_workflow_effect,
        api_workflow_effect_output, api_workflow_revision, api_workflow_revision_restore,
        api_workflow_revisions, api_workflow_run, api_workflow_run_command,
        api_workflow_run_continuations, api_workflow_run_cursors, api_workflow_run_effects,
        api_workflow_run_journal, api_workflow_run_rename, api_workflow_run_replay,
        api_workflow_run_transitions, api_workflow_runs, api_workflow_trigger,
        api_workflow_trigger_backfill, api_workflow_trigger_runs, api_workflow_triggers,
        API_APPROVALS, API_ARTIFACTS_CONTENT, API_CREDENTIALS, API_FREEZE_WINDOWS, API_FUNCTIONS,
        API_FUNCTIONS_CATALOG, API_FUNCTION_ARTIFACTS, API_FUNCTION_EXPORTS, API_IDEMPOTENCY_KEYS,
        API_IDEMPOTENCY_KEYS_CLAIM, API_IDEMPOTENCY_KEYS_COMPLETE, API_IDEMPOTENCY_KEYS_RELEASE,
        API_PACKS_IMPORT, API_PROVIDERS, API_REPLICAS, API_RUNS, API_SCHEDULER_WORKFLOW_RUNS_CLAIM,
        API_SCHEDULER_WORKFLOW_TRIGGER_FIRINGS_CLAIM, API_SUPERVISOR_STATUS, API_WORKFLOWS,
        API_WORKFLOWS_EXPORT, API_WORKFLOWS_IMPORT, API_WORKFLOWS_SIMULATE, API_WORKFLOWS_VALIDATE,
        API_WORKFLOW_EFFECTS, API_WORKFLOW_RUNS, API_WORKFLOW_TRIGGERS_DUE,
        WORKFLOW_JSON_IMPORT_RISK_ACK, WORKFLOW_JSON_IMPORT_RISK_HEADER,
    },
    auth::{
        AgentEnrollmentToken, CreateAgentEnrollmentTokenRequest,
        CreateAgentEnrollmentTokenResponse, EnrollAgentRequest, EnrollAgentResponse,
    },
    billing::ScaleOrgNodesRequest,
    bundles::{Bundle, PackImportResult, ProviderBundle, SecretBundle},
    console::{ConsoleCell, ConsoleSession, ConsoleSessionDetail, NewConsoleCell},
    functions::{
        FunctionAlias, FunctionArtifact, FunctionCatalogEntry, FunctionInvocationTarget,
        FunctionPackage, FunctionPackageDetail, FunctionVersion, NewFunctionVersion,
        ARTIFACT_MEDIA_TYPE,
    },
    orchestration::{
        IdempotencyClaim, IdempotencyClaimRequest, IdempotencyCompleteRequest,
        IdempotencyReleaseRequest, ACTION_IDEMPOTENCY_SCOPE,
    },
    providers::ProviderMetadata,
    provisioning::{NodeBackendsResponse, ProvisionedGroup, ScaleNodesRequest, StopNodeRequest},
    replicas::{
        ReplicaHeartbeatRequest, ReplicaKind, ReplicaListResponse, ReplicaOfflineRequest,
        ReplicaProviderRegistration, ReplicaProviderRegistrationRequest, ReplicaRecord,
        ReplicaRegistrationRequest, ReplicaStatus,
    },
    revisions::{PipelineRevision, WorkflowRevision},
    runs::{RunStatus, RunSummary},
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
};
use uuid::Uuid;

use crate::{
    error::{ApiError, Result},
    locator::ServiceLocator,
    types::{ArtifactContentResponse, RunArtifactPayload, RunChunkPayload, RunStatusPayload},
};

/// Default cap on a single request's total wall-clock time. Bounds a hung or slow web service so a
/// caller (worker/ctl/engine) fails fast and retries rather than parking a task indefinitely.
/// Override with `RUNINATOR_API_TIMEOUT_SECONDS`.
const DEFAULT_REQUEST_TIMEOUT_SECONDS: u64 = 60;

/// Default cap on establishing the TCP/TLS connection, separate from the overall request timeout so a
/// dead host is detected quickly. Override with `RUNINATOR_API_CONNECT_TIMEOUT_SECONDS`.
const DEFAULT_CONNECT_TIMEOUT_SECONDS: u64 = 10;

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

/// Asynchronous API client that wraps `reqwest::Client` and a service locator.
#[derive(Clone)]
pub struct AsyncApiClient<L> {
    client: Client,
    locator: L,
}

impl<L> AsyncApiClient<L>
where
    L: ServiceLocator,
{
    /// List console sessions visible to the authenticated principal.
    pub async fn console_sessions(&self) -> Result<Vec<ConsoleSession>> {
        let url = self.build_url("/console/sessions").await?;
        let response = self.http_get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Vec<ConsoleSession>>().await?)
    }

    /// Create a durable console session.
    pub async fn create_console_session(&self, name: &str) -> Result<ConsoleSession> {
        let url = self.build_url("/console/sessions").await?;
        let response = self
            .http_post(url.clone())
            .json(&json!({ "name": name }))
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<ConsoleSession>().await?)
    }

    /// Fetch a session with its cells and bindings.
    pub async fn console_session(&self, session_id: Uuid) -> Result<ConsoleSessionDetail> {
        let url = self
            .build_url(&format!("/console/sessions/{session_id}"))
            .await?;
        let response = self.http_get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<ConsoleSessionDetail>().await?)
    }

    /// Append a cell to a durable session.
    pub async fn create_console_cell(
        &self,
        session_id: Uuid,
        cell: &NewConsoleCell,
    ) -> Result<ConsoleCell> {
        let url = self
            .build_url(&format!("/console/sessions/{session_id}/cells"))
            .await?;
        let response = self.http_post(url.clone()).json(cell).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<ConsoleCell>().await?)
    }

    /// Run a persisted console cell.
    pub async fn run_console_cell(&self, cell_id: Uuid) -> Result<ConsoleCell> {
        let url = self
            .build_url(&format!("/console/cells/{cell_id}/run"))
            .await?;
        let response = self.http_post(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<ConsoleCell>().await?)
    }

    /// Read and, when terminal, settle a console cell.
    pub async fn console_cell(&self, cell_id: Uuid) -> Result<ConsoleCell> {
        let url = self.build_url(&format!("/console/cells/{cell_id}")).await?;
        let response = self.http_get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<ConsoleCell>().await?)
    }

    /// Cancel the durable workflow behind a running console cell.
    pub async fn cancel_console_cell(&self, cell_id: Uuid) -> Result<TaskResponse> {
        let url = self
            .build_url(&format!("/console/cells/{cell_id}/cancel"))
            .await?;
        let response = self.http_post(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<TaskResponse>().await?)
    }

    pub async fn replay_console_cell(&self, cell_id: Uuid) -> Result<ConsoleCell> {
        let url = self
            .build_url(&format!("/console/cells/{cell_id}/replay"))
            .await?;
        let response = self.http_post(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<ConsoleCell>().await?)
    }

    /// Invoke a packaged function through its generated workflow adapter.
    pub async fn invoke_function(
        &self,
        package: &str,
        export: &str,
        alias: Option<&str>,
        version: Option<i64>,
        input: &Value,
    ) -> Result<Value> {
        let mut path = format!("/functions/{package}/{export}/invocations");
        if let Some(alias) = alias {
            path.push_str(&format!("?alias={alias}"));
        } else if let Some(version) = version {
            path.push_str(&format!("?version={version}"));
        }
        let url = self.build_url(&path).await?;
        let response = self.http_post(url.clone()).json(input).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Value>().await?)
    }

    pub async fn fetch_pipelines(&self) -> Result<Vec<Pipeline>> {
        let url = self.build_url("/pipelines").await?;
        let response = self.http_get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Vec<Pipeline>>().await?)
    }

    pub async fn create_pipeline_run(
        &self,
        pipeline_id: Uuid,
        parameters: Value,
    ) -> Result<PipelineRun> {
        self.create_pipeline_run_at_revision(pipeline_id, parameters, None)
            .await
    }

    pub async fn create_pipeline_run_at_revision(
        &self,
        pipeline_id: Uuid,
        parameters: Value,
        revision: Option<i64>,
    ) -> Result<PipelineRun> {
        let url = self
            .build_url(&format!("/pipelines/{pipeline_id}/runs"))
            .await?;
        let response = self
            .http_post(url.clone())
            .json(&json!({ "parameters": parameters, "revision": revision }))
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<PipelineRun>().await?)
    }

    pub async fn fetch_pipeline_run(&self, run_id: Uuid) -> Result<PipelineRunDetail> {
        let url = self.build_url(&format!("/pipeline_runs/{run_id}")).await?;
        let response = self.http_get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<PipelineRunDetail>().await?)
    }

    pub async fn delete_pipeline_run(&self, run_id: Uuid) -> Result<TaskResponse> {
        let url = self.build_url(&format!("/pipeline_runs/{run_id}")).await?;
        let response = self.http_delete(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<TaskResponse>().await?)
    }

    pub async fn fetch_pipeline(&self, pipeline_id: Uuid) -> Result<Pipeline> {
        let url = self.build_url(&format!("/pipelines/{pipeline_id}")).await?;
        let response = self.http_get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Pipeline>().await?)
    }

    pub async fn upsert_pipeline(&self, pipeline: &Pipeline) -> Result<Pipeline> {
        let url = match pipeline.id {
            Some(id) => self.build_url(&format!("/pipelines/{id}")).await?,
            None => self.build_url("/pipelines").await?,
        };
        let response = match pipeline.id {
            Some(_) => self.http_patch(url.clone()).json(pipeline).send().await?,
            None => self.http_post(url.clone()).json(pipeline).send().await?,
        };
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Pipeline>().await?)
    }

    pub async fn fetch_pipeline_revisions(
        &self,
        pipeline_id: Uuid,
        limit: Option<i64>,
    ) -> Result<Vec<PipelineRevision>> {
        let mut url = self
            .build_url(&format!("/pipelines/{pipeline_id}/revisions"))
            .await?;
        if let Some(limit) = limit {
            url.query_pairs_mut()
                .append_pair("limit", &limit.to_string());
        }
        let response = self.http_get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Vec<PipelineRevision>>().await?)
    }

    pub async fn fetch_pipeline_revision(
        &self,
        pipeline_id: Uuid,
        revision: i64,
    ) -> Result<PipelineRevision> {
        let url = self
            .build_url(&format!("/pipelines/{pipeline_id}/revisions/{revision}"))
            .await?;
        let response = self.http_get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<PipelineRevision>().await?)
    }

    pub async fn delete_pipeline(&self, pipeline_id: Uuid) -> Result<()> {
        let url = self.build_url(&format!("/pipelines/{pipeline_id}")).await?;
        let response = self.http_delete(url.clone()).send().await?;
        Self::handle_response(url, response).await?;
        Ok(())
    }

    pub async fn fetch_pipeline_runs(&self, pipeline_id: Option<Uuid>) -> Result<Vec<PipelineRun>> {
        let path = match pipeline_id {
            Some(pipeline_id) => format!("/pipeline_runs?pipeline_id={pipeline_id}"),
            None => "/pipeline_runs".to_string(),
        };
        let url = self.build_url(&path).await?;
        let response = self.http_get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Vec<PipelineRun>>().await?)
    }

    pub async fn cancel_pipeline_run(&self, run_id: Uuid) -> Result<TaskResponse> {
        let url = self
            .build_url(&format!("/pipeline_runs/{run_id}/cancel"))
            .await?;
        let response = self.http_post(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<TaskResponse>().await?)
    }

    pub async fn pause_pipeline_run(&self, run_id: Uuid) -> Result<TaskResponse> {
        let url = self
            .build_url(&format!("/pipeline_runs/{run_id}/pause"))
            .await?;
        let response =
            Self::handle_response(url.clone(), self.http_post(url).send().await?).await?;
        Ok(response.json::<TaskResponse>().await?)
    }

    pub async fn resume_pipeline_run(&self, run_id: Uuid) -> Result<TaskResponse> {
        let url = self
            .build_url(&format!("/pipeline_runs/{run_id}/resume"))
            .await?;
        let response =
            Self::handle_response(url.clone(), self.http_post(url).send().await?).await?;
        Ok(response.json::<TaskResponse>().await?)
    }

    /// resolve an open `inquire` pause on a pipeline run: continue the pipeline or abort it.
    pub async fn resolve_pipeline_run(
        &self,
        run_id: Uuid,
        decision: &str,
        resolved_by: Option<&str>,
        message: Option<&str>,
    ) -> Result<PipelineRun> {
        let url = self
            .build_url(&format!("/pipeline_runs/{run_id}/resolve"))
            .await?;
        let response = self
            .http_post(url.clone())
            .json(&json!({
                "decision": decision,
                "resolved_by": resolved_by,
                "message": message,
            }))
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<PipelineRun>().await?)
    }

    pub async fn retry_pipeline_member(
        &self,
        run_id: Uuid,
        member_key: &str,
        parameters: Value,
    ) -> Result<runinator_models::pipelines::PipelineMemberAttempt> {
        let mut url = self
            .build_url(&format!("/pipeline_runs/{run_id}/members"))
            .await?;
        url.path_segments_mut()
            .map_err(|_| {
                ApiError::UnexpectedResponse("pipeline retry URL cannot be a base URL".into())
            })?
            .push(member_key)
            .push("retry");
        let response = self
            .http_post(url.clone())
            .json(&json!({ "parameters": parameters }))
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json().await?)
    }

    /// redeem a self-authenticating agent enrollment request. the endpoint is public; this client's
    /// configured API credential, if any, is irrelevant to the proof inside `request`.
    pub async fn enroll_agent(&self, request: &EnrollAgentRequest) -> Result<EnrollAgentResponse> {
        let url = self.build_url("/agents/enroll").await?;
        let response = self.http_post(url.clone()).json(request).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<EnrollAgentResponse>().await?)
    }

    pub async fn create_agent_enrollment_token(
        &self,
        request: &CreateAgentEnrollmentTokenRequest,
    ) -> Result<CreateAgentEnrollmentTokenResponse> {
        let url = self.build_url("/agents/enrollment_tokens").await?;
        let response = self.http_post(url.clone()).json(request).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response
            .json::<CreateAgentEnrollmentTokenResponse>()
            .await?)
    }

    pub async fn list_agent_enrollment_tokens(&self) -> Result<Vec<AgentEnrollmentToken>> {
        let url = self.build_url("/agents/enrollment_tokens").await?;
        let response = self.http_get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Vec<AgentEnrollmentToken>>().await?)
    }

    pub async fn delete_agent_enrollment_token(&self, token_id: &str) -> Result<TaskResponse> {
        let url = self
            .build_url(&format!("/agents/enrollment_tokens/{token_id}"))
            .await?;
        let response = self.http_delete(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<TaskResponse>().await?)
    }

    pub async fn create_agent_directive(
        &self,
        replica_id: Uuid,
        kind: &AgentDirectiveKind,
        expires_in_seconds: Option<u64>,
    ) -> Result<AgentDirectiveRecord> {
        let url = self
            .build_url(&format!("/replicas/{replica_id}/directives"))
            .await?;
        let response = self
            .http_post(url.clone())
            .json(&json!({ "kind": kind, "expires_in_seconds": expires_in_seconds }))
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<AgentDirectiveRecord>().await?)
    }

    pub async fn list_agent_directives(
        &self,
        replica_id: Uuid,
        limit: Option<i64>,
    ) -> Result<Vec<AgentDirectiveRecord>> {
        let mut url = self
            .build_url(&format!("/replicas/{replica_id}/directives"))
            .await?;
        if let Some(limit) = limit {
            url.query_pairs_mut()
                .append_pair("limit", &limit.to_string());
        }
        let response = self.http_get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Vec<AgentDirectiveRecord>>().await?)
    }

    /// Construct a client with default request/connect timeouts applied.
    pub fn new(locator: L) -> reqwest::Result<Self> {
        let client = timed_client_builder().build()?;
        Ok(Self { client, locator })
    }

    /// Construct a client that presents `token` as `Authorization: Bearer …` on every request. A
    /// `None`/empty token yields an unauthenticated client (for stacks with auth disabled). Both
    /// JWTs and API keys are accepted as bearer tokens by the web service.
    pub fn with_credentials(locator: L, token: Option<String>) -> reqwest::Result<Self> {
        let mut builder = timed_client_builder();
        if let Some(token) = token.filter(|t| !t.is_empty()) {
            if let Ok(value) = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}")) {
                let mut headers = reqwest::header::HeaderMap::new();
                headers.insert(reqwest::header::AUTHORIZATION, value);
                builder = builder.default_headers(headers);
            }
        }
        Ok(Self {
            client: builder.build()?,
            locator,
        })
    }

    /// Construct a client using a preconfigured HTTP client instance.
    pub fn with_client(locator: L, client: Client) -> Self {
        Self { client, locator }
    }

    // inject the active w3c trace context (e.g. `traceparent`) into an outbound request so the web
    // service continues this trace. a no-op when otel is off (no headers added). all request helpers
    // below route through this so every outbound call is traced uniformly.
    fn traced(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let mut headers = reqwest::header::HeaderMap::new();
        runinator_observability::telemetry::inject_into_headers(&mut headers);
        builder.headers(headers)
    }

    fn http_get<U: reqwest::IntoUrl>(&self, url: U) -> reqwest::RequestBuilder {
        self.traced(self.client.get(url))
    }

    fn http_post<U: reqwest::IntoUrl>(&self, url: U) -> reqwest::RequestBuilder {
        self.traced(self.client.post(url))
    }

    fn http_patch<U: reqwest::IntoUrl>(&self, url: U) -> reqwest::RequestBuilder {
        self.traced(self.client.patch(url))
    }

    fn http_delete<U: reqwest::IntoUrl>(&self, url: U) -> reqwest::RequestBuilder {
        self.traced(self.client.delete(url))
    }

    /// Fetch provider/action metadata for task authoring.
    pub async fn fetch_providers(&self) -> Result<Vec<ProviderMetadata>> {
        let url = self.build_url(API_PROVIDERS).await?;
        let response = self.http_get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Vec<ProviderMetadata>>().await?)
    }

    /// Register provider/action metadata with the web service.
    pub async fn upsert_provider(&self, provider: &ProviderMetadata) -> Result<ProviderMetadata> {
        let url = self.build_url(API_PROVIDERS).await?;
        let response = self.http_post(url.clone()).json(provider).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<ProviderMetadata>().await?)
    }

    pub async fn register_replica(
        &self,
        request: &ReplicaRegistrationRequest,
    ) -> Result<ReplicaRecord> {
        let url = self.build_url(&format!("{API_REPLICAS}/register")).await?;
        let response = self.http_post(url.clone()).json(request).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<ReplicaRecord>().await?)
    }

    pub async fn heartbeat_replica(
        &self,
        replica_id: Uuid,
        request: &ReplicaHeartbeatRequest,
    ) -> Result<ReplicaRecord> {
        let url = self.build_url(&api_replica_heartbeat(replica_id)).await?;
        let response = self.http_post(url.clone()).json(request).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<ReplicaRecord>().await?)
    }

    pub async fn mark_replica_offline(
        &self,
        replica_id: Uuid,
        request: &ReplicaOfflineRequest,
    ) -> Result<ReplicaRecord> {
        let url = self.build_url(&api_replica_offline(replica_id)).await?;
        let response = self.http_post(url.clone()).json(request).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<ReplicaRecord>().await?)
    }

    pub async fn register_replica_provider(
        &self,
        replica_id: Uuid,
        request: &ReplicaProviderRegistrationRequest,
    ) -> Result<ReplicaProviderRegistration> {
        let url = self.build_url(&api_replica_providers(replica_id)).await?;
        let response = self.http_post(url.clone()).json(request).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<ReplicaProviderRegistration>().await?)
    }

    pub async fn fetch_replica_providers(
        &self,
        replica_id: Uuid,
    ) -> Result<Vec<ReplicaProviderRegistration>> {
        let url = self.build_url(&api_replica_providers(replica_id)).await?;
        let response = self.http_get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Vec<ReplicaProviderRegistration>>().await?)
    }

    pub async fn fetch_replicas(
        &self,
        replica_type: Option<ReplicaKind>,
        status: Option<ReplicaStatus>,
    ) -> Result<ReplicaListResponse> {
        let mut url = self.build_url(API_REPLICAS).await?;
        if let Some(replica_type) = replica_type {
            url.query_pairs_mut()
                .append_pair("replica_type", replica_type.as_str());
        }
        if let Some(status) = status {
            url.query_pairs_mut().append_pair("status", status.as_str());
        }
        let response = self.http_get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<ReplicaListResponse>().await?)
    }

    /// One replica's recent telemetry samples, over the given look-back window in seconds.
    pub async fn fetch_replica_samples(
        &self,
        replica_id: Uuid,
        since_seconds: Option<i64>,
    ) -> Result<ReplicaSampleSeries> {
        let mut url = self
            .build_url(&format!("{API_REPLICAS}/{replica_id}/samples"))
            .await?;
        if let Some(since_seconds) = since_seconds {
            url.query_pairs_mut()
                .append_pair("since_seconds", &since_seconds.to_string());
        }
        let response = self.http_get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<ReplicaSampleSeries>().await?)
    }

    /// list configured node-provisioning backends and the kinds they support.
    pub async fn fetch_node_backends(&self) -> Result<NodeBackendsResponse> {
        let url = self.build_url("/nodes/backends").await?;
        let response = self.http_get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<NodeBackendsResponse>().await?)
    }

    /// list current node groups (desired/available counts) across every backend.
    pub async fn fetch_nodes(&self) -> Result<Vec<ProvisionedGroup>> {
        let url = self.build_url("/nodes").await?;
        let response = self.http_get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Vec<ProvisionedGroup>>().await?)
    }

    /// set the desired node count for a kind on a backend.
    pub async fn scale_nodes(&self, request: &ScaleNodesRequest) -> Result<ProvisionedGroup> {
        let url = self.build_url("/nodes/scale").await?;
        let response = self.http_post(url.clone()).json(request).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<ProvisionedGroup>().await?)
    }

    /// stop/remove a single provisioned node instance.
    pub async fn stop_node(&self, request: &StopNodeRequest) -> Result<Value> {
        let url = self.build_url("/nodes/stop").await?;
        let response = self.http_post(url.clone()).json(request).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Value>().await?)
    }

    /// create an organization; the caller becomes its owner.
    pub async fn create_org(&self, name: &str) -> Result<Value> {
        let url = self.build_url("/orgs").await?;
        let response = self
            .http_post(url.clone())
            .json(&json!({ "name": name }))
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Value>().await?)
    }

    /// list the caller's org memberships (org + role).
    pub async fn list_my_orgs(&self) -> Result<Value> {
        let url = self.build_url("/orgs/me").await?;
        let response = self.http_get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Value>().await?)
    }

    /// an org's dedicated node allocations and projected monthly cost.
    pub async fn fetch_org_nodes(&self, org_id: Uuid) -> Result<Value> {
        let url = self.build_url(&format!("/orgs/{org_id}/nodes")).await?;
        let response = self
            .http_get(url.clone())
            .header("x-org-id", org_id.to_string())
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Value>().await?)
    }

    /// set an org's dedicated node allocation for a (backend, kind); enforced against its quota.
    pub async fn scale_org_nodes(
        &self,
        org_id: Uuid,
        request: &ScaleOrgNodesRequest,
    ) -> Result<Value> {
        let url = self
            .build_url(&format!("/orgs/{org_id}/nodes/scale"))
            .await?;
        let response = self
            .http_post(url.clone())
            .header("x-org-id", org_id.to_string())
            .json(request)
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Value>().await?)
    }

    /// an org's accrued usage and cost over the trailing 30 days.
    pub async fn fetch_org_usage(&self, org_id: Uuid) -> Result<Value> {
        let url = self.build_url(&format!("/orgs/{org_id}/usage")).await?;
        let response = self
            .http_get(url.clone())
            .header("x-org-id", org_id.to_string())
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Value>().await?)
    }

    pub async fn fetch_run(&self, run_id: Uuid) -> Result<RunSummary> {
        let url = self.build_url(&api_run(run_id)).await?;
        let response = self.http_get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<RunSummary>().await?)
    }

    pub async fn fetch_runs_by_status(&self, status: RunStatus) -> Result<Vec<RunSummary>> {
        let url = self
            .build_url(&format!("{API_RUNS}?status={}", status.as_str()))
            .await?;
        let response = self.http_get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Vec<RunSummary>>().await?)
    }

    pub async fn update_run(
        &self,
        run_id: Uuid,
        payload: &RunStatusPayload,
    ) -> Result<TaskResponse> {
        let url = self.build_url(&api_run(run_id)).await?;
        let response = self.http_patch(url.clone()).json(payload).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<TaskResponse>().await?)
    }

    pub async fn append_run_chunk(
        &self,
        run_id: Uuid,
        payload: &RunChunkPayload,
    ) -> Result<TaskResponse> {
        let url = self.build_url(&api_run_chunks(run_id)).await?;
        let response = self.http_post(url.clone()).json(payload).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<TaskResponse>().await?)
    }

    pub async fn add_run_artifact(
        &self,
        run_id: Uuid,
        payload: &RunArtifactPayload,
    ) -> Result<TaskResponse> {
        let url = self.build_url(&api_run_artifacts(run_id)).await?;
        let response = self.http_post(url.clone()).json(payload).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<TaskResponse>().await?)
    }

    pub async fn fetch_workflow(&self, workflow_id: Uuid) -> Result<WorkflowDefinition> {
        let url = self.build_url(&api_workflow(workflow_id)).await?;
        let response = self.http_get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<WorkflowDefinition>().await?)
    }

    pub async fn fetch_workflows(&self) -> Result<Vec<WorkflowDefinition>> {
        let url = self.build_url(API_WORKFLOWS).await?;
        let response = self.http_get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Vec<WorkflowDefinition>>().await?)
    }

    pub async fn fetch_workflow_by_name(&self, name: &str) -> Result<WorkflowDefinition> {
        let mut url = self.build_url(API_WORKFLOWS).await?;
        url.query_pairs_mut().append_pair("name", name);
        let response = self.http_get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<WorkflowDefinition>().await?)
    }

    pub async fn upsert_workflow(
        &self,
        workflow: &WorkflowDefinition,
    ) -> Result<WorkflowDefinition> {
        let url = match workflow.id {
            Some(id) => self.build_url(&api_workflow(id)).await?,
            None => self.build_url(API_WORKFLOWS).await?,
        };
        let response = match workflow.id {
            Some(_) => self.http_patch(url.clone()).json(workflow).send().await?,
            None => self.http_post(url.clone()).json(workflow).send().await?,
        };
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<WorkflowDefinition>().await?)
    }

    /// list a workflow's revision history, newest first.
    pub async fn fetch_workflow_revisions(
        &self,
        workflow_id: Uuid,
        limit: Option<i64>,
    ) -> Result<Vec<WorkflowRevision>> {
        let mut url = self.build_url(&api_workflow_revisions(workflow_id)).await?;
        if let Some(limit) = limit {
            url.query_pairs_mut()
                .append_pair("limit", &limit.to_string());
        }
        let response = self.http_get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Vec<WorkflowRevision>>().await?)
    }

    /// fetch one revision, including the definition it captured.
    pub async fn fetch_workflow_revision(
        &self,
        workflow_id: Uuid,
        revision: i64,
    ) -> Result<WorkflowRevision> {
        let url = self
            .build_url(&api_workflow_revision(workflow_id, revision))
            .await?;
        let response = self.http_get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<WorkflowRevision>().await?)
    }

    /// restore an earlier revision as the workflow's current definition. the restore is saved as a
    /// new revision rather than rewriting history.
    pub async fn restore_workflow_revision(
        &self,
        workflow_id: Uuid,
        revision: i64,
    ) -> Result<WorkflowDefinition> {
        let url = self
            .build_url(&api_workflow_revision_restore(workflow_id, revision))
            .await?;
        let response = self.http_post(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<WorkflowDefinition>().await?)
    }

    /// duplicate a workflow into a new version sharing its name, bumped by `bump`.
    pub async fn duplicate_workflow(
        &self,
        workflow_id: Uuid,
        bump: runinator_models::semver::SemVerBump,
    ) -> Result<WorkflowDefinition> {
        let mut url = self.build_url(&api_workflow_duplicate(workflow_id)).await?;
        url.query_pairs_mut().append_pair("bump", bump.as_str());
        let response = self.http_post(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<WorkflowDefinition>().await?)
    }

    pub async fn validate_workflow(
        &self,
        workflow: &WorkflowDefinition,
    ) -> Result<WorkflowDefinition> {
        let url = self.build_url(API_WORKFLOWS_VALIDATE).await?;
        let response = self.http_post(url.clone()).json(workflow).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<WorkflowDefinition>().await?)
    }

    /// server-side dry-run: walk `request.workflow` with the VM's evaluators against live
    /// config (optionally replaying a prior run), publishing no actions. Returns the raw
    /// `SimulationRun` JSON (status, ordered steps, branch targets, final output).
    pub async fn simulate_workflow(
        &self,
        request: &WorkflowSimulateRequest,
    ) -> Result<serde_json::Value> {
        let url = self.build_url(API_WORKFLOWS_SIMULATE).await?;
        let response = self.http_post(url.clone()).json(request).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<serde_json::Value>().await?)
    }

    /// POST a typed bundle to its associated import endpoint.
    pub async fn import_bundle<B: Bundle>(&self, bundle: &B) -> Result<B> {
        let url = self.build_url(B::RESOURCE).await?;
        let response = self.http_post(url.clone()).json(bundle).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<B>().await?)
    }

    /// POST a raw JSON workflow bundle after acknowledging that system breakage is possible.
    pub async fn import_workflow_bundle(&self, bundle: &WorkflowBundle) -> Result<WorkflowBundle> {
        let url = self.build_url(API_WORKFLOWS_IMPORT).await?;
        let response = self
            .http_post(url.clone())
            .header(
                WORKFLOW_JSON_IMPORT_RISK_HEADER,
                WORKFLOW_JSON_IMPORT_RISK_ACK,
            )
            .json(bundle)
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<WorkflowBundle>().await?)
    }

    /// Build a compiled pack zip (workflows + optional secrets + pipelines) and POST it to
    /// `/packs/import`.
    pub async fn import_pack(
        &self,
        workflows: &WorkflowBundle,
        secrets: Option<&SecretBundle>,
        pipelines: Option<&PipelineBundle>,
        overwrite: bool,
    ) -> Result<PackImportResult> {
        self.import_pack_zip(
            runinator_pack_wire::pack::PackBuilder::new(workflows)
                .secrets(secrets)
                .pipelines(pipelines)
                .build()
                .map_err(|err| ApiError::Pack(err.to_string()))?,
            overwrite,
        )
        .await
    }

    /// Import a pack that also carries packaged functions.
    ///
    /// The artifacts are uploaded by digest *first*, and only the ones the server reports missing
    /// ride in the zip. A pack that carried every artifact every time would push megabytes through
    /// the 10 MB request limit to re-send bytes the server already holds — and the digest is
    /// computed client-side, so asking is cheap.
    pub async fn import_pack_with_functions(
        &self,
        workflows: &WorkflowBundle,
        secrets: Option<&SecretBundle>,
        pipelines: Option<&PipelineBundle>,
        functions: Vec<NewFunctionVersion>,
        artifacts: Vec<(String, Vec<u8>)>,
        overwrite: bool,
    ) -> Result<PackImportResult> {
        let mut builder = runinator_pack_wire::pack::PackBuilder::new(workflows)
            .secrets(secrets)
            .pipelines(pipelines)
            .functions(functions);

        for (digest, bytes) in artifacts {
            if self.fetch_function_artifact(&digest).await?.is_some() {
                continue;
            }

            builder = builder.function_artifact(digest, bytes);
        }

        self.import_pack_zip(
            builder
                .build()
                .map_err(|err| ApiError::Pack(err.to_string()))?,
            overwrite,
        )
        .await
    }

    async fn import_pack_zip(&self, body: Vec<u8>, overwrite: bool) -> Result<PackImportResult> {
        let mut url = self.build_url(API_PACKS_IMPORT).await?;
        if overwrite {
            url.set_query(Some("overwrite=true"));
        }
        let response = self
            .http_post(url.clone())
            .header(reqwest::header::CONTENT_TYPE, "application/zip")
            .body(body)
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<PackImportResult>().await?)
    }

    /// upload artifact bytes and get back the URI to record against them.
    ///
    /// stores bytes only. the artifact row is created by whoever already accounts for the artifact —
    /// for a worker-produced one, the result-event path — so this must not create a second.
    pub async fn upload_artifact_content(
        &self,
        run_id: Uuid,
        name: &str,
        mime_type: &str,
        bytes: Vec<u8>,
    ) -> Result<ArtifactContentResponse> {
        let mut url = self.build_url(API_ARTIFACTS_CONTENT).await?;
        {
            let mut query = url.query_pairs_mut();
            query.append_pair("run_id", &run_id.to_string());
            query.append_pair("name", name);
            query.append_pair("mime_type", mime_type);
        }
        let response = self
            .http_post(url.clone())
            .header(reqwest::header::CONTENT_TYPE, mime_type)
            .body(bytes)
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<ArtifactContentResponse>().await?)
    }

    // ---- packaged functions ----

    /// List published function packages.
    pub async fn fetch_function_packages(&self) -> Result<Vec<FunctionPackage>> {
        let url = self.build_url(API_FUNCTIONS).await?;
        let response = self.http_get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Vec<FunctionPackage>>().await?)
    }

    /// Fetch one package with its versions, aliases, and current exports.
    pub async fn fetch_function_package(&self, package: &str) -> Result<FunctionPackageDetail> {
        let url = self
            .build_url(&format!("{API_FUNCTIONS}/{package}"))
            .await?;
        let response = self.http_get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<FunctionPackageDetail>().await?)
    }

    pub async fn move_function_package(
        &self,
        package_id: Uuid,
        namespace: Option<&str>,
        name: &str,
    ) -> Result<FunctionPackage> {
        let url = self
            .build_url(&format!("/function_packages/{package_id}"))
            .await?;
        let response = self
            .http_patch(url.clone())
            .json(&json!({ "namespace": namespace, "name": name }))
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<FunctionPackage>().await?)
    }

    /// The flattened catalog of every published export.
    pub async fn fetch_function_catalog(&self) -> Result<Vec<FunctionCatalogEntry>> {
        let url = self.build_url(API_FUNCTIONS_CATALOG).await?;
        let response = self.http_get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Vec<FunctionCatalogEntry>>().await?)
    }

    /// Publish one version. The artifact must already be uploaded.
    pub async fn publish_function_version(
        &self,
        request: &NewFunctionVersion,
    ) -> Result<FunctionVersion> {
        let url = self.build_url(API_FUNCTIONS).await?;
        let response = self.http_post(url.clone()).json(request).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<FunctionVersion>().await?)
    }

    /// Delete a package and everything under it.
    pub async fn delete_function_package(&self, package: &str) -> Result<Value> {
        let url = self
            .build_url(&format!("{API_FUNCTIONS}/{package}"))
            .await?;
        let response = self.http_delete(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Value>().await?)
    }

    pub async fn restore_function_package(&self, package: &str) -> Result<Value> {
        let url = self
            .build_url(&format!("{API_FUNCTIONS}/{package}/restore"))
            .await?;
        let response = self.http_post(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Value>().await?)
    }

    /// Point an alias at a version.
    pub async fn set_function_alias(
        &self,
        package: &str,
        alias: &str,
        version: Option<i64>,
        from_alias: Option<&str>,
    ) -> Result<FunctionAlias> {
        let url = self
            .build_url(&format!("{API_FUNCTIONS}/{package}/aliases"))
            .await?;
        let body = json!({ "alias": alias, "version": version, "from_alias": from_alias });
        let response = self.http_post(url.clone()).json(&body).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<FunctionAlias>().await?)
    }

    /// Delete an alias, leaving the version it named untouched.
    pub async fn delete_function_alias(&self, package: &str, alias: &str) -> Result<Value> {
        let url = self
            .build_url(&format!("{API_FUNCTIONS}/{package}/aliases/{alias}"))
            .await?;
        let response = self.http_delete(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Value>().await?)
    }

    /// Resolve one export to the handler, runtime, limits, and digest needed to run it.
    pub async fn resolve_function_export(
        &self,
        export_id: Uuid,
    ) -> Result<FunctionInvocationTarget> {
        let url = self
            .build_url(&format!("{API_FUNCTION_EXPORTS}/{export_id}"))
            .await?;
        let response = self.http_get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<FunctionInvocationTarget>().await?)
    }

    /// Whether the server already holds these bytes.
    ///
    /// This is what makes republishing unchanged code cheap: the digest is computed client-side, so
    /// the upload can be skipped entirely when the answer is yes.
    pub async fn fetch_function_artifact(&self, digest: &str) -> Result<Option<FunctionArtifact>> {
        let url = self
            .build_url(&format!("{API_FUNCTION_ARTIFACTS}/{digest}"))
            .await?;
        let response = self.http_get(url.clone()).send().await?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let response = Self::handle_response(url, response).await?;
        Ok(Some(response.json::<FunctionArtifact>().await?))
    }

    /// Upload package archive bytes under their digest.
    pub async fn upload_function_artifact(
        &self,
        digest: &str,
        bytes: Vec<u8>,
    ) -> Result<FunctionArtifact> {
        let url = self
            .build_url(&format!("{API_FUNCTION_ARTIFACTS}/{digest}"))
            .await?;
        let response = self
            .http_post(url.clone())
            .header(reqwest::header::CONTENT_TYPE, ARTIFACT_MEDIA_TYPE)
            .body(bytes)
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<FunctionArtifact>().await?)
    }

    /// Download a package archive's bytes. This is the worker's fetch path.
    pub async fn download_function_artifact(&self, digest: &str) -> Result<Vec<u8>> {
        let url = self
            .build_url(&format!("{API_FUNCTION_ARTIFACTS}/{digest}/content"))
            .await?;
        let response = self.http_get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.bytes().await?.to_vec())
    }

    pub async fn import_provider_bundle(&self, bundle: &ProviderBundle) -> Result<ProviderBundle> {
        self.import_bundle(bundle).await
    }

    pub async fn import_secret_bundle(&self, bundle: &SecretBundle) -> Result<SecretBundle> {
        self.import_bundle(bundle).await
    }

    pub async fn export_workflow_bundle(
        &self,
        workflow_id: Option<Uuid>,
    ) -> Result<WorkflowBundle> {
        let path = workflow_id
            .map(|id| format!("{}/export", api_workflow(id)))
            .unwrap_or_else(|| API_WORKFLOWS_EXPORT.into());
        let url = self.build_url(&path).await?;
        let response = self.http_get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<WorkflowBundle>().await?)
    }

    pub async fn create_workflow_run(
        &self,
        workflow_id: Uuid,
        parameters: Value,
    ) -> Result<WorkflowRun> {
        self.create_workflow_run_with_options(workflow_id, parameters, false, None)
            .await
    }

    pub async fn create_named_workflow_run(
        &self,
        workflow_id: Uuid,
        parameters: Value,
        name: String,
    ) -> Result<WorkflowRun> {
        self.create_workflow_run_with_options(workflow_id, parameters, false, Some(name))
            .await
    }

    pub async fn fetch_workflow_triggers(&self, workflow_id: Uuid) -> Result<Vec<WorkflowTrigger>> {
        let url = self.build_url(&api_workflow_triggers(workflow_id)).await?;
        let response = self.http_get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Vec<WorkflowTrigger>>().await?)
    }

    pub async fn fetch_due_workflow_triggers(&self) -> Result<Vec<WorkflowTrigger>> {
        let url = self.build_url(API_WORKFLOW_TRIGGERS_DUE).await?;
        let response = self.http_get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Vec<WorkflowTrigger>>().await?)
    }

    /// replay a cron trigger's slots across a past range. slots the loop already fired keep their
    /// original run, so an overlapping range is safe to re-issue.
    pub async fn backfill_workflow_trigger(
        &self,
        trigger_id: Uuid,
        request: &BackfillRequest,
    ) -> Result<BackfillResponse> {
        let url = self
            .build_url(&api_workflow_trigger_backfill(trigger_id))
            .await?;
        let response = self.http_post(url.clone()).json(request).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<BackfillResponse>().await?)
    }

    pub async fn fetch_freeze_windows(&self, active_only: bool) -> Result<Vec<FreezeWindow>> {
        let path = match active_only {
            true => format!("{API_FREEZE_WINDOWS}?active=true"),
            false => API_FREEZE_WINDOWS.to_string(),
        };
        let url = self.build_url(&path).await?;
        let response = self.http_get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Vec<FreezeWindow>>().await?)
    }

    pub async fn create_freeze_window(&self, window: &NewFreezeWindow) -> Result<FreezeWindow> {
        let url = self.build_url(API_FREEZE_WINDOWS).await?;
        let response = self.http_post(url.clone()).json(window).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<FreezeWindow>().await?)
    }

    pub async fn delete_freeze_window(&self, window_id: Uuid) -> Result<TaskResponse> {
        let url = self.build_url(&api_freeze_window(window_id)).await?;
        let response = self.http_delete(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<TaskResponse>().await?)
    }

    pub async fn claim_due_workflow_trigger_firings(
        &self,
        scheduler_id: &str,
        limit: i64,
    ) -> Result<Vec<WorkflowRun>> {
        let url = self
            .build_url(API_SCHEDULER_WORKFLOW_TRIGGER_FIRINGS_CLAIM)
            .await?;
        let response = self
            .http_post(url.clone())
            .json(&json!({ "scheduler_id": scheduler_id, "limit": limit }))
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Vec<WorkflowRun>>().await?)
    }

    pub async fn fetch_workflow_trigger(&self, trigger_id: Uuid) -> Result<WorkflowTrigger> {
        let url = self.build_url(&api_workflow_trigger(trigger_id)).await?;
        let response = self.http_get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<WorkflowTrigger>().await?)
    }

    pub async fn upsert_workflow_trigger(
        &self,
        trigger: &WorkflowTrigger,
    ) -> Result<WorkflowTrigger> {
        let url = match trigger.id {
            Some(id) => self.build_url(&api_workflow_trigger(id)).await?,
            None => {
                self.build_url(&api_workflow_triggers(trigger.workflow_id))
                    .await?
            }
        };
        let response = match trigger.id {
            Some(_) => self.http_patch(url.clone()).json(trigger).send().await?,
            None => self.http_post(url.clone()).json(trigger).send().await?,
        };
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<WorkflowTrigger>().await?)
    }

    pub async fn delete_workflow_trigger(&self, trigger_id: Uuid) -> Result<TaskResponse> {
        let url = self.build_url(&api_workflow_trigger(trigger_id)).await?;
        let response = self.http_delete(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<TaskResponse>().await?)
    }

    pub async fn create_workflow_trigger_run(
        &self,
        trigger_id: Uuid,
        parameters: Value,
        debug: bool,
    ) -> Result<WorkflowRun> {
        let url = self
            .build_url(&api_workflow_trigger_runs(trigger_id))
            .await?;
        let response = self
            .http_post(url.clone())
            .json(&json!({ "parameters": parameters, "debug": debug }))
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        let body = response.json::<Value>().await?;
        serde_json::from_value(
            body.get("run")
                .cloned()
                .ok_or_else(|| ApiError::UnexpectedResponse("missing run".into()))?
                .into(),
        )
        .map_err(|err| ApiError::UnexpectedResponse(err.to_string()))
    }

    pub async fn create_workflow_run_with_debug(
        &self,
        workflow_id: Uuid,
        parameters: Value,
        debug: bool,
    ) -> Result<WorkflowRun> {
        self.create_workflow_run_with_options(workflow_id, parameters, debug, None)
            .await
    }

    pub async fn create_workflow_run_with_options(
        &self,
        workflow_id: Uuid,
        parameters: Value,
        debug: bool,
        name: Option<String>,
    ) -> Result<WorkflowRun> {
        let url = self.build_url(&api_workflow_runs(workflow_id)).await?;
        let response = self
            .http_post(url.clone())
            .json(&json!({ "parameters": parameters, "debug": debug, "name": name }))
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        let body = response.json::<Value>().await?;
        serde_json::from_value(
            body.get("run")
                .cloned()
                .ok_or_else(|| ApiError::UnexpectedResponse("missing run".into()))?
                .into(),
        )
        .map_err(|err| ApiError::UnexpectedResponse(err.to_string()))
    }

    pub async fn fetch_workflow_runs_by_status(
        &self,
        status: WorkflowStatus,
    ) -> Result<Vec<WorkflowRun>> {
        let url = self
            .build_url(&format!("{API_WORKFLOW_RUNS}?status={}", status.as_str()))
            .await?;
        let response = self.http_get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Vec<WorkflowRun>>().await?)
    }

    pub async fn claim_workflow_runs_for_scheduler(
        &self,
        scheduler_id: &str,
        statuses: &[WorkflowStatus],
        lease_until: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<WorkflowRun>> {
        let url = self.build_url(API_SCHEDULER_WORKFLOW_RUNS_CLAIM).await?;
        let response = self
            .http_post(url.clone())
            .json(&json!({
                "scheduler_id": scheduler_id,
                "statuses": statuses,
                "lease_until": lease_until,
                "limit": limit
            }))
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Vec<WorkflowRun>>().await?)
    }

    pub async fn renew_workflow_run_claim(
        &self,
        workflow_run_id: Uuid,
        scheduler_id: &str,
        lease_until: DateTime<Utc>,
    ) -> Result<TaskResponse> {
        let url = self
            .build_url(&api_scheduler_workflow_run_claim_renew(workflow_run_id))
            .await?;
        let response = self
            .http_post(url.clone())
            .json(&json!({ "scheduler_id": scheduler_id, "lease_until": lease_until }))
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<TaskResponse>().await?)
    }

    pub async fn release_workflow_run_claim(
        &self,
        workflow_run_id: Uuid,
        scheduler_id: &str,
    ) -> Result<TaskResponse> {
        let url = self
            .build_url(&api_scheduler_workflow_run_claim_release(workflow_run_id))
            .await?;
        let response = self
            .http_post(url.clone())
            .json(&json!({ "scheduler_id": scheduler_id }))
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<TaskResponse>().await?)
    }

    pub async fn fetch_workflow_runs(
        &self,
        status: Option<WorkflowStatus>,
        workflow_id: Option<Uuid>,
    ) -> Result<Vec<WorkflowRun>> {
        let mut url = self.build_url(API_WORKFLOW_RUNS).await?;
        if let Some(status) = status {
            url.query_pairs_mut().append_pair("status", status.as_str());
        }
        if let Some(workflow_id) = workflow_id {
            url.query_pairs_mut()
                .append_pair("workflow_id", &workflow_id.to_string());
        }
        let response = self.http_get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Vec<WorkflowRun>>().await?)
    }

    pub async fn fetch_workflow_runs_by_name(
        &self,
        name: &str,
        open_only: bool,
    ) -> Result<Vec<WorkflowRun>> {
        let mut url = self.build_url(API_WORKFLOW_RUNS).await?;
        url.query_pairs_mut()
            .append_pair("name", name)
            .append_pair("open", if open_only { "true" } else { "false" });
        let response = self.http_get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Vec<WorkflowRun>>().await?)
    }

    pub async fn update_workflow_run(
        &self,
        workflow_run_id: Uuid,
        status: WorkflowStatus,
        active_node_id: Option<String>,
        state: Option<Value>,
        message: Option<String>,
    ) -> Result<TaskResponse> {
        let url = self.build_url(&api_workflow_run(workflow_run_id)).await?;
        let response = self
            .http_patch(url.clone())
            .json(&json!({
                "status": status,
                "active_node_id": active_node_id,
                "state": state,
                "message": message
            }))
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<TaskResponse>().await?)
    }

    pub async fn rename_workflow_run(
        &self,
        workflow_run_id: Uuid,
        name: Option<String>,
    ) -> Result<TaskResponse> {
        let url = self
            .build_url(&api_workflow_run_rename(workflow_run_id))
            .await?;
        let response = self
            .http_post(url.clone())
            .json(&json!({ "name": name }))
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<TaskResponse>().await?)
    }

    pub async fn pause_workflow_run(&self, workflow_run_id: Uuid) -> Result<TaskResponse> {
        self.post_workflow_run_command(workflow_run_id, "pause")
            .await
    }

    pub async fn resume_workflow_run(&self, workflow_run_id: Uuid) -> Result<TaskResponse> {
        self.post_workflow_run_command(workflow_run_id, "resume")
            .await
    }

    pub async fn cancel_workflow_run(&self, workflow_run_id: Uuid) -> Result<TaskResponse> {
        self.post_workflow_run_command(workflow_run_id, "cancel")
            .await
    }

    pub async fn replay_workflow_run(
        &self,
        workflow_run_id: Uuid,
        from_step_id: Option<String>,
    ) -> Result<WorkflowRun> {
        let url = self
            .build_url(&api_workflow_run_replay(workflow_run_id))
            .await?;
        let response = self
            .http_post(url.clone())
            .json(&json!({ "from_step_id": from_step_id }))
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        let body = response.json::<Value>().await?;
        serde_json::from_value(
            body.get("run")
                .cloned()
                .ok_or_else(|| ApiError::UnexpectedResponse("missing run".into()))?
                .into(),
        )
        .map_err(|err| ApiError::UnexpectedResponse(err.to_string()))
    }

    async fn post_workflow_run_command(
        &self,
        workflow_run_id: Uuid,
        command: &str,
    ) -> Result<TaskResponse> {
        let url = self
            .build_url(&api_workflow_run_command(workflow_run_id, command))
            .await?;
        let response = self.http_post(url.clone()).json(&json!({})).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<TaskResponse>().await?)
    }

    pub async fn fetch_workflow_run(&self, workflow_run_id: Uuid) -> Result<WorkflowRun> {
        let url = self.build_url(&api_workflow_run(workflow_run_id)).await?;
        let response = self.http_get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        let body = response.json::<Value>().await?;
        serde_json::from_value(
            body.get("run")
                .cloned()
                .ok_or_else(|| ApiError::UnexpectedResponse("missing run".into()))?
                .into(),
        )
        .map_err(|err| ApiError::UnexpectedResponse(err.to_string()).into())
    }

    pub async fn delete_workflow_run(&self, workflow_run_id: Uuid) -> Result<TaskResponse> {
        let url = self
            .build_url(&format!("/workflow_runs/{workflow_run_id}"))
            .await?;
        let response = self.http_delete(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<TaskResponse>().await?)
    }

    /// Read VM execution branches for a workflow run.  This is the successor to graph-cursor and
    /// node-run reads for compiled runs.
    pub async fn fetch_workflow_continuations(
        &self,
        workflow_run_id: Uuid,
    ) -> Result<Vec<WorkflowContinuation>> {
        let url = self
            .build_url(&api_workflow_run_continuations(workflow_run_id))
            .await?;
        let response =
            Self::handle_response(url.clone(), self.http_get(url.clone()).send().await?).await?;
        Ok(response.json::<Vec<WorkflowContinuation>>().await?)
    }

    pub async fn fetch_workflow_continuation(
        &self,
        continuation_id: Uuid,
    ) -> Result<WorkflowContinuation> {
        let url = self
            .build_url(&api_workflow_continuation(continuation_id))
            .await?;
        let response =
            Self::handle_response(url.clone(), self.http_get(url.clone()).send().await?).await?;
        Ok(response.json::<WorkflowContinuation>().await?)
    }

    pub async fn fetch_workflow_effects(
        &self,
        workflow_run_id: Uuid,
    ) -> Result<Vec<WorkflowEffect>> {
        let url = self
            .build_url(&api_workflow_run_effects(workflow_run_id))
            .await?;
        let response =
            Self::handle_response(url.clone(), self.http_get(url.clone()).send().await?).await?;
        Ok(response.json::<Vec<WorkflowEffect>>().await?)
    }

    pub async fn fetch_workflow_effect(&self, effect_id: Uuid) -> Result<WorkflowEffect> {
        let url = self.build_url(&api_workflow_effect(effect_id)).await?;
        let response =
            Self::handle_response(url.clone(), self.http_get(url.clone()).send().await?).await?;
        Ok(response.json::<WorkflowEffect>().await?)
    }

    pub async fn fetch_workflow_effect_output(
        &self,
        effect_id: Uuid,
    ) -> Result<Vec<WorkflowEffectOutputEvent>> {
        let url = self
            .build_url(&api_workflow_effect_output(effect_id))
            .await?;
        let response =
            Self::handle_response(url.clone(), self.http_get(url.clone()).send().await?).await?;
        Ok(response.json::<Vec<WorkflowEffectOutputEvent>>().await?)
    }

    pub async fn settle_workflow_effect(
        &self,
        effect_id: Uuid,
        status: WorkflowEffectStatus,
        output: Option<Value>,
        message: Option<String>,
    ) -> Result<TaskResponse> {
        let url = self
            .build_url(&format!("{}/settle", api_workflow_effect(effect_id)))
            .await?;
        let response = self
            .http_post(url.clone())
            .json(&json!({ "status": status, "output": output, "message": message }))
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<TaskResponse>().await?)
    }

    pub async fn fetch_workflow_journal(
        &self,
        workflow_run_id: Uuid,
    ) -> Result<Vec<WorkflowJournalRecord>> {
        let url = self
            .build_url(&api_workflow_run_journal(workflow_run_id))
            .await?;
        let response =
            Self::handle_response(url.clone(), self.http_get(url.clone()).send().await?).await?;
        Ok(response.json::<Vec<WorkflowJournalRecord>>().await?)
    }

    /// Render graph markers from the persisted continuation IP and the immutable source map.
    pub async fn fetch_workflow_vm_cursors(
        &self,
        workflow_run_id: Uuid,
    ) -> Result<Vec<WorkflowVmCursor>> {
        let url = self
            .build_url(&api_workflow_run_cursors(workflow_run_id))
            .await?;
        let response =
            Self::handle_response(url.clone(), self.http_get(url.clone()).send().await?).await?;
        Ok(response.json::<Vec<WorkflowVmCursor>>().await?)
    }

    pub async fn fetch_workflow_run_transitions(
        &self,
        workflow_run_id: Uuid,
    ) -> Result<Vec<runinator_models::orchestration::NodeTransition>> {
        let url = self
            .build_url(&api_workflow_run_transitions(workflow_run_id))
            .await?;
        let response = self.http_get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response
            .json::<Vec<runinator_models::orchestration::NodeTransition>>()
            .await?)
    }

    /// download an artifact's raw bytes from the streaming download endpoint.
    pub async fn download_artifact(&self, artifact_id: Uuid) -> Result<Vec<u8>> {
        let url = self.build_url(&api_artifact_download(artifact_id)).await?;
        let response = self.http_get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.bytes().await?.to_vec())
    }

    pub async fn fetch_supervisor_status(&self) -> Result<Value> {
        let url = self.build_url(API_SUPERVISOR_STATUS).await?;
        let response = self.http_get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Value>().await?)
    }

    pub async fn fetch_approvals(&self, workflow_run_id: Option<Uuid>) -> Result<Vec<Value>> {
        let mut url = self.build_url(API_APPROVALS).await?;
        if let Some(workflow_run_id) = workflow_run_id {
            url.query_pairs_mut()
                .append_pair("workflow_run_id", &workflow_run_id.to_string());
        }
        let response = self.http_get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Vec<Value>>().await?)
    }

    pub async fn settle_approval_effect(
        &self,
        effect_id: Uuid,
        approved: bool,
        _resolved_by: Option<String>,
        message: Option<String>,
        output_json: Option<Value>,
    ) -> Result<Value> {
        let url = self
            .build_url(&format!("{API_WORKFLOW_EFFECTS}/{effect_id}/settle"))
            .await?;
        let response = self
            .http_post(url.clone())
            .json(&json!({
                "status": if approved { "succeeded" } else { "failed" },
                "message": message,
                "output": output_json
            }))
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Value>().await?)
    }

    pub async fn create_automation_record(&self, path: &str, record: Value) -> Result<Value> {
        let url = self.build_url(path).await?;
        let response = self.http_post(url.clone()).json(&record).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Value>().await?)
    }

    pub async fn fetch_idempotency_key(&self, scope: &str, key: &str) -> Result<Option<Value>> {
        let url = self
            .build_url(&format!("{API_IDEMPOTENCY_KEYS}?scope={scope}&key={key}"))
            .await?;
        let response = self.http_get(url.clone()).send().await?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let response = Self::handle_response(url, response).await?;
        Ok(Some(response.json::<Value>().await?))
    }

    /// reserve an action node's idempotency key before invoking its provider. the reply says whether
    /// this node run may execute, must replay an already-recorded result, or lost to another claimant.
    pub async fn claim_idempotency_key(
        &self,
        key: &str,
        owner_node_run_id: Uuid,
        lease_seconds: i64,
    ) -> Result<IdempotencyClaim> {
        let url = self.build_url(API_IDEMPOTENCY_KEYS_CLAIM).await?;
        let response = self
            .http_post(url.clone())
            .json(&IdempotencyClaimRequest {
                scope: ACTION_IDEMPOTENCY_SCOPE.into(),
                key: key.to_string(),
                owner_node_run_id,
                lease_seconds,
            })
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<IdempotencyClaim>().await?)
    }

    /// record this node run's terminal outcome against the key it reserved, so a redelivery replays
    /// it instead of re-invoking the provider. `Ok(false)` means the reservation was no longer ours.
    pub async fn complete_idempotency_key(
        &self,
        key: &str,
        owner_node_run_id: Uuid,
        result: Value,
    ) -> Result<bool> {
        let url = self.build_url(API_IDEMPOTENCY_KEYS_COMPLETE).await?;
        let response = self
            .http_post(url.clone())
            .json(&IdempotencyCompleteRequest {
                scope: ACTION_IDEMPOTENCY_SCOPE.into(),
                key: key.to_string(),
                owner_node_run_id,
                result,
            })
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<TaskResponse>().await?.success)
    }

    /// free an unfinished reservation after a non-success outcome, so a retry is not held off.
    pub async fn release_idempotency_key(
        &self,
        key: &str,
        owner_node_run_id: Uuid,
    ) -> Result<bool> {
        let url = self.build_url(API_IDEMPOTENCY_KEYS_RELEASE).await?;
        let response = self
            .http_post(url.clone())
            .json(&IdempotencyReleaseRequest {
                scope: ACTION_IDEMPOTENCY_SCOPE.into(),
                key: key.to_string(),
                owner_node_run_id,
            })
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<TaskResponse>().await?.success)
    }

    pub async fn put_idempotency_key(
        &self,
        scope: &str,
        key: &str,
        result: Value,
    ) -> Result<Value> {
        let url = self.build_url(API_IDEMPOTENCY_KEYS).await?;
        let response = self
            .http_post(url.clone())
            .json(&json!({ "scope": scope, "key": key, "result": result }))
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Value>().await?)
    }

    pub async fn fetch_credential(&self, scope: &str, name: &str) -> Result<String> {
        let mut url = self.build_url(API_CREDENTIALS).await?;
        url.query_pairs_mut()
            .append_pair("scope", scope)
            .append_pair("name", name);
        let response = self.http_get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        let body = response.json::<Value>().await?;
        body.get("secret")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| ApiError::UnexpectedResponse("missing credential secret".into()))
    }

    /// Fetch a secret through its durable logical identity. UUID-backed workflow bindings use
    /// this path so moving the human-readable scope/name alias cannot break a queued action.
    pub async fn fetch_credential_by_id(&self, id: Uuid) -> Result<String> {
        let url = self.build_url(&format!("/credentials/{id}")).await?;
        let response = self.http_get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        let body = response.json::<Value>().await?;
        body.get("secret")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| ApiError::UnexpectedResponse("missing credential secret".into()))
    }

    /// Record execution metadata for a scheduled task run.
    pub async fn log_task_run(&self) -> Result<TaskResponse> {
        Err(ApiError::UnexpectedResponse("deprecated".into()))
    }

    async fn build_url(&self, path: &str) -> Result<Url> {
        let base = self
            .locator
            .wait_for_service_url()
            .await
            .map_err(ApiError::discovery)?;
        let base_url = Url::parse(&base).map_err(|source| ApiError::InvalidBaseUrl {
            url: base.clone(),
            source,
        })?;
        let trimmed_path = path.trim_start_matches('/');
        base_url
            .join(trimmed_path)
            .map_err(|source| ApiError::InvalidPath {
                base: base_url.clone(),
                path: trimmed_path.to_string(),
                source,
            })
    }

    async fn handle_response(url: Url, response: Response) -> Result<Response> {
        let status = response.status();
        if status.is_success() {
            Ok(response)
        } else {
            let message = response
                .text()
                .await
                .unwrap_or_else(|_| "<unable to read body>".into());
            Err(ApiError::Http {
                status,
                url,
                message,
            })
        }
    }
}
