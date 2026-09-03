import { command, isTauriRuntime } from "./runtime";
import { apiBaseUrl, httpAuthToken, setHttpAuthToken } from "./httpRuntime";
import { asJsonRecord } from "../domain/json";
import type {
  JsonRecord,
  JsonValue,
  ApiKey,
  AuthSessionSummary,
  ChangePasswordInput,
  CreatePersonalApiKeyInput,
  PersonalApiKeySecret,
  PersonalApiKeyScope,
  UpdateCurrentUserInput,
  AgentEnrollmentToken,
  AgentMachineEnrollment,
  AgentDirectiveKind,
  AgentDirectiveRecord,
  CreateAgentEnrollmentTokenInput,
  CreateAgentEnrollmentTokenResponse,
  CreateApiKeyResponse,
  ConsoleCell,
  ConsoleSession,
  ConsoleSessionDetail,
  CredentialSummary,
  CredentialDetail,
  FunctionArtifact,
  FunctionCatalogEntry,
  FunctionPackage,
  FunctionPackageDetail,
  FunctionVersion,
  NewFunctionVersion,
  DevPackApplyResult,
  BackfillRequest,
  BackfillResponse,
  CalendarScope,
  CalendarSubscriptionSecret,
  DevPackInspectResult,
  DevPackTextFile,
  FreezeWindow,
  GateRecord,
  Grant,
  NodeTransition,
  NodeTransitionStat,
  NewFreezeWindow,
  NewNotificationPolicy,
  Notification,
  NotificationDelivery,
  NotificationPolicy,
  PermissionLevel,
  PrincipalType,
  ProviderMetadata,
  WorkflowNodeKindMetadata,
  WorkflowTriggerKindMetadata,
  EnumCatalogMetadata,
  ReplicaListResponse,
  ReplicaProviderRegistration,
  ReplicaRecord,
  RunSummary,
  SettingKind,
  ServiceStatus,
  TaskResponse,
  Team,
  User,
  RexRapCompletionRequest,
  RexRapCompletionResponse,
  RexRapDiagnostic,
  RexRapHoverRequest,
  RexRapHoverResponse,
  WorkflowBundle,
  WorkflowDefinition,
  WorkflowRevision,
  WorkflowRunCreated,
  WorkflowRunArtifact,
  WorkflowFile,
  WorkflowRunDetail,
  WorkflowNodeRun,
  WorkflowContinuation,
  WorkflowEffect,
  WorkflowEffectOutputEvent,
  WorkflowJournalRecord,
  WorkflowVmCursor,
  WorkflowSimulateRequest,
  WorkflowTrigger,
  SimulationRun,
  Pipeline,
  PipelineMemberAttempt,
  PipelineRun,
  PipelineRunDetail,
  PipelineTrigger,
  IngressAdmission,
  IngressInboxEntry,
} from "../domain/models";

async function fetchIngressJson<T>(path: string, init?: RequestInit): Promise<T> {
  const token = httpAuthToken();
  const headers = new Headers(init?.headers);

  if (init?.body) {
    headers.set("content-type", "application/json");
  }

  if (token) {
    headers.set("authorization", `Bearer ${token}`);
  }

  const response = await fetch(`${apiBaseUrl()}/${path}`, {
    ...init,
    headers,
  });

  if (!response.ok) {
    const body = (await response.json().catch(() => ({}))) as { message?: string };

    throw new Error(body.message ?? `Ingress request failed (${String(response.status)})`);
  }

  return response.json() as Promise<T>;
}

export async function fetchIngressAdmission(scope: string, correlationKey: string) {
  const query = new URLSearchParams({ scope, correlation_key: correlationKey });
  return fetchIngressJson<IngressAdmission>(`ingress/admission?${query}`);
}

export async function fetchIngressTimeline(scope: string, correlationKey: string) {
  const query = new URLSearchParams({ scope, correlation_key: correlationKey });
  return fetchIngressJson<IngressInboxEntry[]>(`ingress/admission/events?${query}`);
}

export interface ScopeRefInput {
  kind: "platform" | "organization" | "team" | "user";
  id?: string | null;
}

export async function listExternalIngressControl(params: URLSearchParams) {
  const query = params.toString();
  return fetchIngressJson<JsonRecord[]>(`ingress_control/external${query ? `?${query}` : ""}`);
}

export async function configureExternalIngressGate(
  targetKind: "workflow" | "pipeline",
  targetId: string,
  mode: "disabled" | "paused" | "review",
) {
  return fetchIngressJson<JsonRecord>(`ingress_control/targets/${targetKind}/${targetId}/gate`, {
    method: "PUT",
    body: JSON.stringify({ mode }),
  });
}

export async function approveExternalIngress(id: string) {
  return fetchIngressJson<JsonRecord>(`ingress_control/external/${id}/approve`, { method: "POST" });
}

export async function dropExternalIngress(id: string) {
  return fetchIngressJson<JsonRecord>(`ingress_control/external/${id}/drop`, { method: "POST" });
}

export async function releaseExternalIngress(
  targetKind: "workflow" | "pipeline",
  targetId: string,
) {
  return fetchIngressJson<JsonRecord[]>(
    `ingress_control/targets/${targetKind}/${targetId}/release`,
    {
      method: "POST",
    },
  );
}

export async function listBrokerIngressControl(params: URLSearchParams) {
  const query = params.toString();
  return fetchIngressJson<JsonRecord[]>(`ingress_control/broker${query ? `?${query}` : ""}`);
}

export async function fetchBrokerIngressSession(scope: ScopeRefInput) {
  const params = new URLSearchParams({ scope_kind: scope.kind });

  if (scope.id) {
    params.set("scope_id", scope.id);
  }

  return fetchIngressJson<JsonRecord>(`ingress_control/broker/session?${params}`);
}

export async function configureBrokerIngressSession(
  scope: ScopeRefInput,
  mode: "off" | "observe" | "hold_orchestration_nudges",
) {
  return fetchIngressJson<JsonRecord>("ingress_control/broker/session", {
    method: "PUT",
    body: JSON.stringify({ scope, mode }),
  });
}

export async function renewBrokerIngressSession(scope: ScopeRefInput) {
  return fetchIngressJson<JsonRecord>("ingress_control/broker/session/heartbeat", {
    method: "POST",
    body: JSON.stringify({ scope }),
  });
}

export async function approveBrokerIngress(id: string) {
  return fetchIngressJson<JsonRecord>(`ingress_control/broker/${id}/approve`, { method: "POST" });
}

export async function dropBrokerIngress(id: string) {
  return fetchIngressJson<JsonRecord>(`ingress_control/broker/${id}/drop`, { method: "POST" });
}

export interface WorkflowRexRapSaveRequest {
  source: string;
  enabled: boolean;
  workflow_id?: string | null;
  triggers?: WorkflowTrigger[];
  ui?: JsonRecord | null;
}

export interface ForeignLanguageRuntimeConfig {
  image: string;
  setup_script: string;
  environment?: Record<string, string>;
  toolchain?: {
    executable: string;
    build_args: string[];
    run_args: string[];
  };
  limits?: {
    memory_mb: number;
    cpu_millis: number;
    pids: number;
    tmpfs_mb: number;
    max_output_bytes: number;
  };
}

const FOREIGN_LANGUAGE_SCOPE = "foreign_languages";

export interface AuthConfigResponse {
  enabled: boolean;
}

export interface LoginResult {
  access_token: string;
  refresh_token: string;
  expires_in: number;
  user: JsonRecord;
  assignments: JsonRecord[];
  effective_actions: string[];
}

export async function fetchAuthConfig() {
  return command<AuthConfigResponse>("auth_config");
}

export async function fetchAuthMe() {
  return command<JsonRecord>("auth_me");
}

export async function updateCurrentUser(request: UpdateCurrentUserInput) {
  return command<User>("update_current_user", { request });
}

export async function changeCurrentPassword(request: ChangePasswordInput) {
  return command<TaskResponse>("change_current_password", { request });
}

export async function listCurrentSessions() {
  return command<AuthSessionSummary[]>("list_current_sessions");
}

export async function revokeCurrentSession(sessionId: string) {
  return command<TaskResponse>("revoke_current_session", { sessionId });
}

export async function revokeOtherSessions() {
  return command<TaskResponse>("revoke_other_sessions");
}

export async function listPersonalApiKeys() {
  return command<ApiKey[]>("list_personal_api_keys");
}

export async function listPersonalApiKeyScopes() {
  return command<PersonalApiKeyScope[]>("list_personal_api_key_scopes");
}

export async function createPersonalApiKey(request: CreatePersonalApiKeyInput) {
  return command<PersonalApiKeySecret>("create_personal_api_key", { request });
}

export async function login(username: string, password: string) {
  return command<LoginResult>("login", { username, password });
}

export async function refreshSession(refreshToken: string) {
  return command<LoginResult>("refresh_session", { refreshToken });
}

export async function logout(refreshToken: string) {
  return command<TaskResponse>("logout", { refreshToken });
}

export interface AuthSettings {
  max_refreshes: number;
}

export async function fetchAuthSettings() {
  return command<AuthSettings>("fetch_auth_settings");
}

export async function saveAuthSettings(maxRefreshes: number) {
  return command<AuthSettings>("save_auth_settings", { maxRefreshes });
}

export interface ServerSettingDefinition {
  key: string;
  section: string;
  label: string;
  description: string;
  unit: string;
  kind?: "integer" | "boolean";
  default: number;
  minimum: number;
  maximum: number;
  usual_minimum: number;
  usual_maximum: number;
}

export type ServerSettingValue = number | boolean;
export type ServerSettingsValues = Record<string, Record<string, ServerSettingValue>>;

export interface ServerSettingsResponse {
  values: ServerSettingsValues;
  catalog: ServerSettingDefinition[];
  runtime_catalog?: RuntimeSettingDefinition[];
}

export interface RuntimeSettingDefinition {
  key: string;
  section: string;
  label: string;
  description: string;
  value: string;
  source: string;
  restart_required: boolean;
  sensitive: boolean;
}

export async function fetchServerSettings() {
  return command<ServerSettingsResponse>("fetch_server_settings");
}

export async function saveServerSettings(settings: ServerSettingsValues) {
  return command<ServerSettingsResponse>("save_server_settings", { settings });
}

// push the access token to both runtimes: the web fetch layer and (on desktop) the tauri client.
export async function setAccessToken(token: string | null) {
  setHttpAuthToken(token);

  if (isTauriRuntime()) {
    await command<TaskResponse>("set_access_token", { token });
  }
}

export async function listResourceGrants(resourceType: string, resourceId: string) {
  return command<JsonRecord[]>("list_resource_grants", { resourceType, resourceId });
}

export async function createResourceGrant(
  resourceType: string,
  resourceId: string,
  principalType: "user" | "team",
  principalId: string,
  permission: "view" | "run" | "edit" | "own",
) {
  return command<JsonRecord>("create_resource_grant", {
    resourceType,
    resourceId,
    principalType,
    principalId,
    permission,
  });
}

export async function revokeResourceGrant(
  resourceType: string,
  resourceId: string,
  grantId: string,
) {
  return command<TaskResponse>("revoke_resource_grant", { resourceType, resourceId, grantId });
}

export async function transferResourceOwner(
  resourceType:
    | "workflow"
    | "pipeline"
    | "function_package"
    | "console_session"
    | "setting"
    | "execution_profile"
    | "orchestration_adapter"
    | "library_file"
    | "notification_policy",
  resourceId: string,
  scopeKind: "platform" | "organization" | "team" | "user",
  scopeId: string | null,
) {
  return command<JsonRecord>("transfer_resource_owner", {
    resourceType,
    resourceId,
    scopeKind,
    scopeId,
  });
}

export async function fetchResourceOwner(resourceType: string, resourceId: string) {
  return command<JsonRecord>("fetch_resource_owner", { resourceType, resourceId });
}

export interface CreateUserInput {
  username: string;
  password: string;
  email?: string | null;
  platform_role: "admin" | "operator" | "auditor" | "member";
}

export interface UpdateUserInput {
  email?: string | null;
  password?: string | null;
  platform_role?: "admin" | "operator" | "auditor" | "member" | null;
  disabled?: boolean | null;
}

export interface CreateApiKeyInput {
  name: string;
  principal_kind: "user" | "service";
  principal_id: string;
  system_role?: "engine" | "worker" | "waker" | "agent" | "replica" | null;
  org_id?: string | null;
  action_ceiling?: string[];
  expires_at?: string | null;
}

export interface UpdateApiKeyInput {
  name?: string | null;
  expires_at?: string | null;
  disabled?: boolean | null;
}

export async function listUsers() {
  return command<User[]>("list_users");
}

export async function createUser(request: CreateUserInput) {
  return command<User>("create_user", { request });
}

export async function updateUser(userId: string, request: UpdateUserInput) {
  return command<User>("update_user", { userId, request });
}

export async function deleteUser(userId: string) {
  return command<TaskResponse>("delete_user", { userId });
}

export async function listTeams() {
  return command<Team[]>("list_teams");
}

export async function createTeam(name: string) {
  return command<Team>("create_team", { name });
}

export async function updateTeam(teamId: string, name: string) {
  return command<Team>("update_team", { teamId, name });
}

export async function deleteTeam(teamId: string) {
  return command<TaskResponse>("delete_team", { teamId });
}

export async function listTeamMembers(teamId: string) {
  return command<User[]>("list_team_members", { teamId });
}

export async function listUserTeams(userId: string) {
  return command<Team[]>("list_user_teams", { userId });
}

export async function addTeamMember(
  teamId: string,
  userId: string,
  role: "owner" | "admin" | "operator" | "member",
) {
  return command<TaskResponse>("add_team_member", { teamId, userId, role });
}

export async function removeTeamMember(teamId: string, userId: string) {
  return command<TaskResponse>("remove_team_member", { teamId, userId });
}

export async function listApiKeys() {
  return command<ApiKey[]>("list_api_keys");
}

export async function createApiKey(request: CreateApiKeyInput) {
  return command<CreateApiKeyResponse>("create_api_key", { request });
}

export async function updateApiKey(keyId: string, request: UpdateApiKeyInput) {
  return command<ApiKey>("update_api_key", { keyId, request });
}

export async function revokeApiKey(keyId: string) {
  return command<TaskResponse>("revoke_api_key", { keyId });
}

export async function listDeadLetters(channel?: string, limit?: number) {
  return command<JsonRecord[]>("list_dead_letters", { channel, limit });
}

export interface BrokerMessageFilter extends Record<string, unknown> {
  workflowRunId?: string;
  pipelineRunId?: string;
  channel?: string;
  limit?: number;
}

export async function listBrokerMessages(filter: BrokerMessageFilter = {}) {
  return command<JsonRecord[]>("list_broker_messages", filter);
}

export async function listAuditLog(actorId?: string, action?: string, limit?: number) {
  return command<JsonRecord[]>("list_audit_log", { actorId, action, limit });
}

export async function rotateApiKey(keyId: string) {
  return command<CreateApiKeyResponse>("rotate_api_key", { keyId });
}

export async function grantResourceAccess(
  resourceType: string,
  workflowId: string,
  principalType: PrincipalType,
  principalId: string,
  permission: PermissionLevel,
) {
  return command<Grant>("create_resource_grant", {
    resourceType,
    resourceId: workflowId,
    principalType,
    principalId,
    permission,
  });
}

export async function getServiceStatus() {
  return command<ServiceStatus>("get_service_status");
}

export async function startServiceDiscovery() {
  return command("start_service_discovery");
}

export async function fetchWorkflowRunArtifacts(workflowRunId: string) {
  // Workflow VM output is the sole source of artifact history after the VM cutover. The former
  // `/workflow_runs/{id}/artifacts` endpoint read the removed workflow_run_artifacts table.
  const effects = await fetchWorkflowEffects(workflowRunId);
  const output = await Promise.all(effects.map((effect) => fetchWorkflowEffectOutput(effect.id)));

  return output.flatMap((events) =>
    events.flatMap((event) => {
      if (event.output.type !== "artifact") {
        return [];
      }

      const artifact = asJsonRecord(event.output.artifact);
      return [
        {
          id: event.event_id,
          workflow_run_id: event.workflow_run_id,
          // VM effects, rather than node-run rows, own this output. The effect id is the durable
          // execution identity that lets an operator correlate the artifact to the VM debugger.
          node_id: event.effect_id,
          // A VM artifact is addressed by its effect-output event and URI; legacy run_artifact ids
          // no longer exist, so the old `/artifacts/{id}/download` endpoint cannot serve it.
          artifact_id: null,
          name: typeof artifact.name === "string" ? artifact.name : "artifact",
          mime_type:
            typeof artifact.mime_type === "string"
              ? artifact.mime_type
              : "application/octet-stream",
          size_bytes: typeof artifact.size_bytes === "number" ? artifact.size_bytes : 0,
          uri: typeof artifact.uri === "string" ? artifact.uri : "",
          metadata: asJsonRecord(artifact.metadata),
          created_at: new Date(event.created_at * 1000).toISOString(),
        } satisfies WorkflowRunArtifact,
      ];
    }),
  );
}

export async function fetchWorkflowContinuations(workflowRunId: string) {
  return command<WorkflowContinuation[]>("fetch_workflow_continuations", { workflowRunId });
}

export async function fetchWorkflowEffects(workflowRunId: string) {
  return command<WorkflowEffect[]>("fetch_workflow_effects", { workflowRunId });
}

export async function fetchWorkflowEffectOutput(effectId: string) {
  return command<WorkflowEffectOutputEvent[]>("fetch_workflow_effect_output", { effectId });
}

export async function settleWorkflowEffect(
  effectId: string,
  status: "succeeded" | "failed" | "rejected" | "timed_out" | "canceled",
  output: JsonValue | null = null,
  message: string | null = null,
) {
  return command<TaskResponse>("settle_workflow_effect", { effectId, status, output, message });
}

export async function fetchWorkflowJournal(workflowRunId: string) {
  return command<WorkflowJournalRecord[]>("fetch_workflow_journal", { workflowRunId });
}

export async function fetchWorkflowVmCursors(workflowRunId: string) {
  return command<WorkflowVmCursor[]>("fetch_workflow_vm_cursors", { workflowRunId });
}

// The edges a single run actually walked, in order, reconstructed from the immutable VM journal.
export async function fetchWorkflowRunTransitions(workflowRunId: string) {
  return command<NodeTransition[]>("fetch_workflow_run_transitions", { workflowRunId });
}

// aggregated `from -> to` edges out of one node across every run of its workflow.
export async function fetchWorkflowNodeTransitions(workflowId: string, nodeId: string) {
  return command<NodeTransitionStat[]>("fetch_workflow_node_transitions", { workflowId, nodeId });
}

export async function fetchWorkflows() {
  return command<WorkflowDefinition[]>("fetch_workflows");
}

export async function saveWorkflow(workflow: WorkflowDefinition) {
  return command<WorkflowDefinition>("save_workflow", { workflow });
}

export async function saveWorkflowBundle(request: WorkflowBundle) {
  return command<WorkflowBundle>("save_workflow_bundle", { request });
}

export async function simulateWorkflow(request: WorkflowSimulateRequest) {
  return command<SimulationRun>("simulate_workflow", { request });
}

export async function saveWorkflowRexRap(request: WorkflowRexRapSaveRequest) {
  return command<WorkflowBundle>("save_workflow_rexrap", { request });
}

export async function compileRexRap(source: string, enabled: boolean) {
  return command<WorkflowDefinition>("compile_rexrap", { source, enabled });
}

export async function analyzeRexRap(source: string, sourcePath?: string | null) {
  return command<RexRapDiagnostic[]>("analyze_rexrap", { source, sourcePath: sourcePath ?? null });
}

export async function completeRexRap(request: RexRapCompletionRequest) {
  return command<RexRapCompletionResponse>("complete_rexrap", { request });
}

export async function hoverRexRap(request: RexRapHoverRequest) {
  return command<RexRapHoverResponse | null>("hover_rexrap", { request });
}

export async function formatRexRap(source: string) {
  return command<string>("format_rexrap", { source });
}

export async function decompileToRexRap(workflow: WorkflowDefinition) {
  return command<string>("decompile_to_rexrap", { workflow });
}

export async function evaluateExpression(expression: unknown, context: unknown) {
  return command<unknown>("evaluate_expression", { expression, context });
}

function requireTauriDevPack() {
  if (!isTauriRuntime()) {
    throw new Error("Dev pack file access is only available in the Tauri desktop app.");
  }
}

export async function inspectDevPack(path: string, skipSettings = false) {
  requireTauriDevPack();
  return command<DevPackInspectResult>("inspect_dev_pack", { path, skipSettings });
}

export async function readDevPackFile(path: string) {
  requireTauriDevPack();
  return command<DevPackTextFile>("read_dev_pack_file", { path });
}

export async function writeDevPackFile(path: string, content: string) {
  requireTauriDevPack();
  return command<DevPackTextFile>("write_dev_pack_file", { path, content });
}

export async function applyDevPack(path: string, skipSettings = false) {
  requireTauriDevPack();
  return command<DevPackApplyResult>("apply_dev_pack", { path, skipSettings });
}

export async function deleteWorkflow(workflowId: string) {
  return command<TaskResponse>("delete_workflow", { workflowId });
}

export async function duplicateWorkflow(
  workflowId: string,
  bump: "major" | "minor" | "patch" = "minor",
) {
  return command<WorkflowDefinition>("duplicate_workflow", { workflowId, bump });
}

export async function fetchWorkflowRevisions(workflowId: string, limit?: number) {
  return command<WorkflowRevision[]>("fetch_workflow_revisions", { workflowId, limit });
}

export async function fetchWorkflowRevision(workflowId: string, revision: number) {
  return command<WorkflowRevision>("fetch_workflow_revision", { workflowId, revision });
}

export async function restoreWorkflowRevision(workflowId: string, revision: number) {
  return command<WorkflowDefinition>("restore_workflow_revision", { workflowId, revision });
}

export async function fetchWorkflowTriggers(workflowId: string) {
  return command<WorkflowTrigger[]>("fetch_workflow_triggers", { workflowId });
}

export async function saveWorkflowTrigger(trigger: WorkflowTrigger, creating: boolean) {
  return command<WorkflowTrigger>("save_workflow_trigger", { trigger, creating });
}

export async function deleteWorkflowTrigger(triggerId: string) {
  return command<TaskResponse>("delete_workflow_trigger", { triggerId });
}

// triggers whose next execution has come due. the scheduler reads this too; the console exposes it
// so an operator can see what is about to fire.
export async function fetchDueTriggers() {
  return command<WorkflowTrigger[]>("fetch_due_triggers", {});
}

// fire a trigger by hand, which creates a run exactly as the schedule would have.
export async function createTriggerRun(triggerId: string, parameters: unknown = {}, debug = false) {
  return command<JsonRecord>("create_trigger_run", { triggerId, parameters, debug });
}

// one workflow's bundle, or the whole set when no id is given.
export async function exportWorkflowBundle(workflowId?: string) {
  return command<WorkflowBundle>("export_workflow_bundle", { workflowId: workflowId ?? null });
}

export async function fetchPipelines() {
  return command<Pipeline[]>("fetch_pipelines");
}

export async function fetchPipeline(pipelineId: string) {
  return command<Pipeline>("fetch_pipeline", { pipelineId });
}

export async function savePipeline(pipeline: Pipeline) {
  return command<Pipeline>("save_pipeline", { pipeline });
}

export async function deletePipeline(pipelineId: string) {
  return command<TaskResponse>("delete_pipeline", { pipelineId });
}

// reassign a pipeline's owning organization; null makes it platform-global.
export async function setPipelineOwner(pipelineId: string, orgId: string | null) {
  await transferResourceOwner(
    "pipeline",
    pipelineId,
    orgId == null ? "platform" : "organization",
    orgId,
  );
  return fetchPipeline(pipelineId);
}

export async function fetchPipelineTriggers(pipelineId: string) {
  return command<PipelineTrigger[]>("fetch_pipeline_triggers", { pipelineId });
}

export async function savePipelineTrigger(trigger: PipelineTrigger, creating: boolean) {
  return command<PipelineTrigger>("save_pipeline_trigger", { trigger, creating });
}

export async function deletePipelineTrigger(triggerId: string) {
  return command<TaskResponse>("delete_pipeline_trigger", { triggerId });
}

export async function createPipelineRun(pipelineId: string, parameters: unknown = {}) {
  return command<PipelineRun>("create_pipeline_run", { pipelineId, parameters });
}

export async function fetchPipelineRuns() {
  return command<PipelineRun[]>("fetch_pipeline_runs");
}

export async function fetchOrchestrations(filters: Record<string, unknown> = {}) {
  return command<import("../domain/models").OrchestrationBinding[]>("fetch_orchestrations", {
    filters,
  });
}

export async function fetchOrchestration(orchestrationId: string) {
  return command<import("../domain/models").OrchestrationBinding>("fetch_orchestration", {
    orchestrationId,
  });
}

export async function fetchOrchestrationEpochs(orchestrationId: string) {
  return command<import("../domain/models").OrchestrationEpoch[]>("fetch_orchestration_epochs", {
    orchestrationId,
  });
}

export async function fetchOrchestrationEvents(orchestrationId: string) {
  return command<import("../domain/models").OrchestrationReduction[]>(
    "fetch_orchestration_events",
    {
      orchestrationId,
    },
  );
}

export async function fetchOrchestrationEvidence(orchestrationId: string) {
  return command<import("../domain/models").OrchestrationEvidence[]>(
    "fetch_orchestration_evidence",
    {
      orchestrationId,
    },
  );
}

export async function fetchOrchestrationCommands(orchestrationId: string) {
  return command<import("../domain/models").OrchestrationCommand[]>(
    "fetch_orchestration_commands",
    {
      orchestrationId,
    },
  );
}

export async function fetchOrchestrationWorkspaces(orchestrationId: string) {
  return command<import("../domain/models").WorkspaceLease[]>("fetch_orchestration_workspaces", {
    orchestrationId,
  });
}

export async function fetchOrchestrationAliases(orchestrationId: string) {
  return command<import("../domain/models").OrchestrationCorrelationAlias[]>(
    "fetch_orchestration_aliases",
    { orchestrationId },
  );
}

export async function addOrchestrationAlias(
  orchestrationId: string,
  source: string,
  scope: string,
  correlationKey: string,
) {
  return command<import("../domain/models").OrchestrationCorrelationAlias>(
    "add_orchestration_alias",
    { orchestrationId, source, scope, correlationKey },
  );
}

export async function deleteOrchestrationAlias(orchestrationId: string, aliasId: string) {
  return command<TaskResponse>("delete_orchestration_alias", { orchestrationId, aliasId });
}

export async function fetchExternalOperations(orchestrationId: string) {
  return command<import("../domain/models").ExternalOperation[]>("fetch_external_operations", {
    orchestrationId,
  });
}

export async function resolveExternalOperation(
  orchestrationId: string,
  operationId: string,
  resolution: "succeeded" | "failed" | "retry",
  reason: string,
  receipt: unknown = null,
) {
  return command<import("../domain/models").ExternalOperation>("resolve_external_operation", {
    orchestrationId,
    operationId,
    resolution,
    reason,
    receipt,
  });
}

export async function fetchAdapterKinds() {
  return command<import("../domain/models").AdapterKindCatalogEntry[]>("fetch_adapter_kinds");
}

export async function fetchAdapters() {
  return command<import("../domain/models").AdapterDefinition[]>("fetch_adapters");
}

export async function fetchAdapter(adapterId: string) {
  return command<import("../domain/models").AdapterDefinition>("fetch_adapter", { adapterId });
}

export async function fetchAdapterRevisions(adapterId: string) {
  return command<import("../domain/models").AdapterRevision[]>("fetch_adapter_revisions", {
    adapterId,
  });
}

export async function fetchAdapterPollStatus(adapterId: string) {
  return command<import("../domain/models").AdapterPollStatus>("fetch_adapter_poll_status", {
    adapterId,
  });
}

export interface AdapterApplyInput {
  name: string;
  kind: string;
  kind_version: string;
  transport: "webhook" | "polling";
  configuration: unknown;
  secret_bindings: Record<string, string>;
  identity_configuration: unknown;
  expected_revision?: number;
}

export async function applyAdapter(adapter: AdapterApplyInput, adapterId?: string) {
  return command<import("../domain/models").AdapterDefinition>("apply_adapter", {
    adapter,
    adapterId,
  });
}

export async function setAdapterEnabled(adapterId: string, enabled: boolean) {
  return command<import("../domain/models").AdapterDefinition>("set_adapter_enabled", {
    adapterId,
    enabled,
  });
}

export async function deleteAdapter(adapterId: string) {
  return command("delete_adapter", { adapterId });
}

export async function testAdapter(
  adapterId: string,
  headers: Record<string, string>,
  bodyBase64: string,
) {
  return command<unknown>("test_adapter", { adapterId, headers, bodyBase64 });
}

export async function fetchAdapterHealth() {
  return command<unknown>("fetch_adapter_health");
}

export async function reloadAdapterHost() {
  return command<unknown>("reload_adapter_host");
}

export async function sendOrchestrationIntent(
  orchestrationId: string,
  intent: string,
  reason: string,
  payload: unknown = {},
  idempotencyKey: string = crypto.randomUUID(),
) {
  return command("send_orchestration_intent", {
    orchestrationId,
    intent,
    reason,
    payload,
    idempotencyKey,
  });
}

export async function requeueOrchestration(
  orchestrationId: string,
  reason: string,
  idempotencyKey: string = crypto.randomUUID(),
) {
  return command<import("../domain/models").OrchestrationBinding>("requeue_orchestration", {
    orchestrationId,
    reason,
    idempotencyKey,
  });
}

export async function fetchPipelineRun(pipelineRunId: string) {
  return command<PipelineRunDetail>("fetch_pipeline_run", { pipelineRunId });
}

export async function deletePipelineRun(pipelineRunId: string) {
  return command<TaskResponse>("delete_pipeline_run", { pipelineRunId });
}

export interface ManagedRunOverrideOptions {
  reason: string;
  idempotencyKey: string;
}

export async function cancelPipelineRun(
  pipelineRunId: string,
  override?: ManagedRunOverrideOptions,
) {
  return command<TaskResponse>("cancel_pipeline_run", {
    pipelineRunId,
    overrideReason: override?.reason ?? null,
    idempotencyKey: override?.idempotencyKey ?? null,
  });
}

export async function pausePipelineRun(
  pipelineRunId: string,
  override?: ManagedRunOverrideOptions,
) {
  return command<TaskResponse>("pause_pipeline_run", {
    pipelineRunId,
    overrideReason: override?.reason ?? null,
    idempotencyKey: override?.idempotencyKey ?? null,
  });
}

export async function resumePipelineRun(
  pipelineRunId: string,
  override?: ManagedRunOverrideOptions,
) {
  return command<TaskResponse>("resume_pipeline_run", {
    pipelineRunId,
    overrideReason: override?.reason ?? null,
    idempotencyKey: override?.idempotencyKey ?? null,
  });
}

export async function retryPipelineMember(
  pipelineRunId: string,
  memberKey: string,
  parameters: unknown = {},
  override?: ManagedRunOverrideOptions,
) {
  return command<PipelineMemberAttempt>("retry_pipeline_member", {
    pipelineRunId,
    memberKey,
    parameters,
    overrideReason: override?.reason ?? null,
    idempotencyKey: override?.idempotencyKey ?? null,
  });
}

// resolve a pipeline run's pending inquiry (a member with the `inquire` failure mode paused it).
// `continue` fires the failed member's onward pipeline links and resumes; `abort` settles the run
// `failed` now.
export async function resolvePipelineRun(
  pipelineRunId: string,
  decision: "continue" | "abort",
  resolvedBy?: string | null,
  message?: string | null,
) {
  return command<PipelineRun>("resolve_pipeline_run", {
    pipelineRunId,
    decision,
    resolvedBy: resolvedBy ?? null,
    message: message ?? null,
  });
}

export async function createWorkflowRun(
  workflowId: string,
  options: { debug?: boolean; parameters?: unknown; fileIds?: string[] } = {},
) {
  return command<WorkflowRunCreated>("create_workflow_run", {
    workflowId,
    debug: Boolean(options.debug),
    parameters: options.parameters ?? {},
    fileIds: options.fileIds ?? [],
  });
}

export async function fetchWorkflowRuns(workflowId?: string) {
  return command<RunSummary[]>("fetch_workflow_runs", { workflowId });
}

/**
 * The web service projects an effect's source-map location when it can, but journal order is a
 * second, durable way to recover that association.  It is particularly important while a run is
 * live: a continuation cursor moves on after an effect settles, so it cannot describe the node
 * that issued a historical effect.
 */
interface JournalEffectNode {
  nodeId: string;
  journalEntryId: string;
  sequence: number;
  createdAt: number;
}

function journalEffectNodes(journal: WorkflowJournalRecord[]): Map<string, JournalEffectNode> {
  const lastNodeByContinuation = new Map<string, JournalEffectNode>();
  const pendingEffectByContinuation = new Map<string, string>();
  const nodeByEffect = new Map<string, JournalEffectNode>();

  for (const record of [...journal].sort((left, right) => left.sequence - right.sequence)) {
    const entry = asJsonRecord(record.entry);
    const continuationId =
      record.continuation_id ??
      (typeof entry.continuation_id === "string" ? entry.continuation_id : null);

    if (entry.type === "node_entered" && continuationId && typeof entry.node_id === "string") {
      const node = {
        nodeId: entry.node_id,
        journalEntryId: record.id,
        sequence: record.sequence,
        createdAt: record.created_at,
      };
      lastNodeByContinuation.set(continuationId, node);
      // `suspend_on_effect` persists its durable EffectRequested boundary before it appends the
      // node entries collected while interpreting that boundary.  Associate each of those later
      // entries with the outstanding effect; the final one is the node that issued it.  Keeping
      // this update also preserves compatibility with older journals that recorded node entries
      // first, since an effect is initially associated with the most recent entry below.
      const pendingEffectId = pendingEffectByContinuation.get(continuationId);

      if (pendingEffectId) {
        nodeByEffect.set(pendingEffectId, node);
      }

      continue;
    }

    if (entry.type === "effect_settled" && continuationId && typeof entry.effect_id === "string") {
      if (pendingEffectByContinuation.get(continuationId) === entry.effect_id) {
        pendingEffectByContinuation.delete(continuationId);
      }

      continue;
    }

    if (
      entry.type !== "effect_requested" ||
      !continuationId ||
      typeof entry.effect_id !== "string"
    ) {
      continue;
    }

    pendingEffectByContinuation.set(continuationId, entry.effect_id);
    const nodeId = lastNodeByContinuation.get(continuationId);

    if (nodeId) {
      nodeByEffect.set(entry.effect_id, nodeId);
    }
  }

  return nodeByEffect;
}

function workflowEffectRequest(effect: WorkflowEffect): JsonRecord {
  return typeof effect.request === "object" && effect.request !== null
    ? (effect.request as JsonRecord)
    : {};
}

/** Earliest durable journal boundary for each effect, used to order same-second executions. */
function journalEffectSequences(journal: WorkflowJournalRecord[]): Map<string, number> {
  const sequences = new Map<string, number>();

  for (const record of journal) {
    const entry = asJsonRecord(record.entry);
    const effectId =
      record.effect_id ?? (typeof entry.effect_id === "string" ? entry.effect_id : null);

    if (effectId && !sequences.has(effectId)) {
      sequences.set(effectId, record.sequence);
    }
  }

  return sequences;
}

function usableTimestamp(value: string | null | undefined): boolean {
  return typeof value === "string" && Number.isFinite(Date.parse(value));
}

/** Prefer a real timestamp over a legacy empty string or malformed value. */
function mergeTimestamp(
  materialized: string | null | undefined,
  projected: string | null | undefined,
): string | null | undefined {
  if (usableTimestamp(materialized)) {
    return materialized;
  }

  if (usableTimestamp(projected)) {
    return projected;
  }

  return materialized ?? projected;
}

function projectedEffectNodeId(
  effect: WorkflowEffect,
  cursorByContinuation: Map<string, WorkflowVmCursor>,
  journalNodesByEffect: Map<string, JournalEffectNode>,
): string | null {
  return (
    effect.node_id ??
    journalNodesByEffect.get(effect.id)?.nodeId ??
    (["requested", "running", "input_required"].includes(effect.status)
      ? cursorByContinuation.get(effect.continuation_id)?.node_id
      : null) ??
    null
  );
}

/**
 * A transitional server can return a partially materialized `nodes` array alongside the newer VM
 * effect history. Treat neither as complete: retain the server rows with their precise timing, and
 * add every projected effect/journal row it omitted. Matching effect ids are merged so the timeline
 * does not show the same execution twice.
 */
function mergeWorkflowRunNodes(
  materialized: WorkflowNodeRun[],
  projected: WorkflowNodeRun[],
): WorkflowNodeRun[] {
  const merged = [...materialized];
  const indexById = new Map(merged.map((node, index) => [node.id, index] as const));
  const indexByEffectId = new Map(
    merged.flatMap((node, index) => {
      const state = node.state;
      const effectId = state?.effect_id;
      return state !== undefined &&
        typeof effectId === "string" &&
        state.journal_entry_id === undefined
        ? ([[effectId, index]] as const)
        : [];
    }),
  );
  const untimedIndexByNodeId = new Map<string, number[]>();

  for (const [index, node] of merged.entries()) {
    if (usableTimestamp(node.created_at) || usableTimestamp(node.started_at)) {
      continue;
    }

    const indexes = untimedIndexByNodeId.get(node.node_id) ?? [];
    indexes.push(index);
    untimedIndexByNodeId.set(node.node_id, indexes);
  }

  for (const node of projected) {
    const state = node.state;
    const effectId =
      state !== undefined &&
      typeof state.effect_id === "string" &&
      state.journal_entry_id === undefined
        ? state.effect_id
        : null;
    const matchingUntimedRows = untimedIndexByNodeId.get(node.node_id) ?? [];
    const matchingUntimedIndex =
      matchingUntimedRows.length === 1 ? matchingUntimedRows[0] : undefined;
    const existingIndex =
      indexById.get(node.id) ??
      (effectId ? indexByEffectId.get(effectId) : undefined) ??
      matchingUntimedIndex;

    if (existingIndex === undefined) {
      const index = merged.length;
      merged.push(node);
      indexById.set(node.id, index);

      if (effectId) {
        indexByEffectId.set(effectId, index);
      }

      continue;
    }

    const existing = merged[existingIndex];
    untimedIndexByNodeId.delete(existing.node_id);
    merged[existingIndex] = {
      ...node,
      ...existing,
      state: { ...(node.state ?? {}), ...(existing.state ?? {}) },
      parameters:
        Object.keys(existing.parameters).length > 0 ? existing.parameters : node.parameters,
      output_json: existing.output_json ?? node.output_json,
      created_at: mergeTimestamp(existing.created_at, node.created_at) ?? undefined,
      started_at: mergeTimestamp(existing.started_at, node.started_at),
      finished_at: mergeTimestamp(existing.finished_at, node.finished_at),
      message: existing.message ?? node.message,
    };
  }

  return merged.sort((left, right) => {
    const leftAt = Date.parse(left.created_at ?? "");
    const rightAt = Date.parse(right.created_at ?? "");

    if (Number.isFinite(leftAt) && Number.isFinite(rightAt) && leftAt !== rightAt) {
      return leftAt - rightAt;
    }

    return left.id.localeCompare(right.id);
  });
}

export async function fetchWorkflowRun(workflowRunId: string): Promise<WorkflowRunDetail> {
  // The detail itself is the operator's source of truth. VM history enriches it when available,
  // but an older or briefly unavailable side endpoint must never turn a valid run into a blank
  // inspector (and erase the timeline/Gantt in the process).
  const detail = await command<WorkflowRunDetail>("fetch_workflow_run", { workflowRunId });
  const [continuations, effects, journal, vmCursors] = await Promise.all([
    fetchWorkflowContinuations(workflowRunId).catch(() => []),
    fetchWorkflowEffects(workflowRunId).catch(() => []),
    fetchWorkflowJournal(workflowRunId).catch(() => []),
    fetchWorkflowVmCursors(workflowRunId).catch(() => []),
  ]);
  const cursorByContinuation = new Map(
    vmCursors.map((cursor) => [cursor.continuation_id, cursor] as const),
  );
  const journalNodesByEffect = journalEffectNodes(journal);
  const journalSequencesByEffect = journalEffectSequences(journal);
  const journalNodeByEffect = new Map<string, JournalEffectNode>();
  const effectJournalEntryIds = new Set<string>();

  for (const effect of [...effects].sort(
    (left, right) =>
      (journalSequencesByEffect.get(left.id) ?? left.sequence) -
      (journalSequencesByEffect.get(right.id) ?? right.sequence),
  )) {
    const nodeId = projectedEffectNodeId(effect, cursorByContinuation, journalNodesByEffect);
    const journalNode = journalNodesByEffect.get(effect.id);

    if (
      !nodeId ||
      journalNode?.nodeId !== nodeId ||
      effectJournalEntryIds.has(journalNode.journalEntryId)
    ) {
      continue;
    }

    journalNodeByEffect.set(effect.id, journalNode);
    effectJournalEntryIds.add(journalNode.journalEntryId);
  }

  const failedNodeIds = new Set(
    journal.flatMap((record) => {
      const entry = asJsonRecord(record.entry);
      return entry.type === "failed" && typeof entry.node_id === "string" ? [entry.node_id] : [];
    }),
  );
  const enteredNodes: WorkflowNodeRun[] = journal.flatMap((record) => {
    const entry = asJsonRecord(record.entry);

    if (
      entry.type !== "node_entered" ||
      typeof entry.node_id !== "string" ||
      effectJournalEntryIds.has(record.id)
    ) {
      return [];
    }

    const timestamp = new Date(record.created_at * 1000).toISOString();
    return [
      {
        id: record.id,
        workflow_run_id: record.workflow_run_id,
        node_id: entry.node_id,
        status: failedNodeIds.has(entry.node_id) ? "failed" : "succeeded",
        attempt: 0,
        parameters: {},
        state: {
          journal_entry_id: record.id,
          node_entered_journal_id: record.id,
          timeline_sequence: record.sequence,
        },
        cursor_id: record.continuation_id ?? null,
        created_at: timestamp,
        started_at: timestamp,
        finished_at: timestamp,
        message: null,
      },
    ];
  });
  const retryNodes: WorkflowNodeRun[] = journal.flatMap((record) => {
    const entry = asJsonRecord(record.entry);

    if (
      entry.type !== "effect_retry_scheduled" ||
      typeof entry.effect_id !== "string" ||
      typeof entry.attempt !== "number" ||
      !Number.isFinite(entry.attempt)
    ) {
      return [];
    }

    const effect = effects.find((candidate) => candidate.id === entry.effect_id);

    if (!effect) {
      return [];
    }

    const nodeId = projectedEffectNodeId(effect, cursorByContinuation, journalNodesByEffect);

    if (!nodeId) {
      return [];
    }

    const timestamp = new Date(record.created_at * 1000).toISOString();
    const availableAt =
      typeof entry.available_at === "number"
        ? new Date(entry.available_at * 1000).toISOString()
        : null;
    const attempt = Math.floor(entry.attempt);
    return [
      {
        id: record.id,
        workflow_run_id: record.workflow_run_id,
        node_id: nodeId,
        status: "retrying",
        attempt,
        parameters: workflowEffectRequest(effect),
        state: {
          effect_id: effect.id,
          journal_entry_id: record.id,
          timeline_sequence: record.sequence,
          retry_available_at: entry.available_at ?? null,
        },
        cursor_id: effect.continuation_id,
        created_at: timestamp,
        started_at: timestamp,
        finished_at: timestamp,
        message: availableAt
          ? `Retry attempt ${String(attempt)} scheduled for ${availableAt}`
          : `Retry attempt ${String(attempt)} scheduled`,
      },
    ];
  });
  const effectNodes: WorkflowNodeRun[] = effects.flatMap((effect) => {
    // VM effects retain their originating graph node through the journal projection. A live
    // cursor is only the last fallback: it moves after an effect settles and cannot describe the
    // historical node that ran. The journal fallback keeps the canvas correct when a lightweight
    // websocket update races the server's source-map projection.
    const nodeId = projectedEffectNodeId(effect, cursorByContinuation, journalNodesByEffect);

    if (!nodeId) {
      return [];
    }

    const request = workflowEffectRequest(effect);
    const journalNode = journalNodeByEffect.get(effect.id);
    const requestType = typeof request.type === "string" ? request.type : "";
    const status =
      effect.status === "requested" ||
      effect.status === "running" ||
      effect.status === "input_required"
        ? requestType === "approval"
          ? "approval_required"
          : ["input", "signal", "gate", "event_wait"].includes(requestType)
            ? "waiting"
            : effect.status
        : effect.status;
    return [
      {
        id: effect.id,
        workflow_run_id: effect.workflow_run_id,
        node_id: nodeId,
        status,
        attempt: effect.attempt,
        parameters: request,
        output_json: effect.result ?? null,
        state: {
          ...request,
          effect_id: effect.id,
          effect_receipt_id: effect.id,
          ...(journalNode ? { node_entered_journal_id: journalNode.journalEntryId } : {}),
          timeline_sequence: Math.min(
            journalNode?.sequence ?? Number.POSITIVE_INFINITY,
            journalSequencesByEffect.get(effect.id) ?? effect.sequence,
          ),
        },
        cursor_id: effect.continuation_id,
        created_at: new Date((journalNode?.createdAt ?? effect.created_at) * 1000).toISOString(),
        started_at: new Date((journalNode?.createdAt ?? effect.created_at) * 1000).toISOString(),
        finished_at: effect.finished_at ? new Date(effect.finished_at * 1000).toISOString() : null,
        message: effect.message ?? null,
      },
    ];
  });
  return {
    ...detail,
    // Mixed-version servers can materialize only infrastructure steps while the VM endpoints own
    // action history. Merge both sources or ordinary actions disappear from the graph, step log,
    // timeline, and Gantt whenever one materialized row happens to be present.
    nodes: mergeWorkflowRunNodes(detail.nodes, [...enteredNodes, ...retryNodes, ...effectNodes]),
    continuations,
    effects,
    journal,
    vm_cursors: vmCursors,
    execution_state: {
      ...(detail.execution_state ?? {}),
      cursors: vmCursors
        .map((cursor) => ({
          id: cursor.continuation_id,
          node_id: cursor.node_id ?? "",
          debug: { paused: cursor.status === "paused" },
        }))
        .filter((cursor) => cursor.node_id.length > 0),
    },
  } satisfies WorkflowRunDetail;
}

export async function deleteWorkflowRun(workflowRunId: string) {
  return command<TaskResponse>("delete_workflow_run", { workflowRunId });
}

export async function stepWorkflowRun(workflowRunId: string, cursor?: string | null) {
  return command<TaskResponse>("step_workflow_run", { workflowRunId, cursor });
}

export async function continueWorkflowRun(workflowRunId: string, cursor?: string | null) {
  return command<TaskResponse>("continue_workflow_run", { workflowRunId, cursor });
}

export async function setWorkflowRunBreakpoints(workflowRunId: string, breakpoints: string[]) {
  return command<TaskResponse>("set_workflow_run_breakpoints", {
    workflowRunId,
    breakpoints,
  });
}

export async function runWorkflowToNode(workflowRunId: string, cursor: string, nodeId: string) {
  return command<TaskResponse>("run_workflow_to_node", { workflowRunId, cursor, nodeId });
}

export async function setWorkflowRunPauseOnFailure(workflowRunId: string, enabled: boolean) {
  return command<TaskResponse>("set_workflow_run_pause_on_failure", {
    workflowRunId,
    enabled,
  });
}

export async function cancelWorkflowRun(
  workflowRunId: string,
  override?: ManagedRunOverrideOptions,
) {
  return command<TaskResponse>("cancel_workflow_run", {
    workflowRunId,
    overrideReason: override?.reason ?? null,
    idempotencyKey: override?.idempotencyKey ?? null,
  });
}

export type WorkflowTerminalControl =
  | { type: "input"; data: string }
  | { type: "resize"; cols: number; rows: number }
  | { type: "eof" };

export async function controlWorkflowEffectTerminal(
  effectId: string,
  control: WorkflowTerminalControl,
) {
  return command<TaskResponse>("control_workflow_effect_terminal", { effectId, control });
}

export async function pauseWorkflowRun(
  workflowRunId: string,
  override?: ManagedRunOverrideOptions,
) {
  return command<TaskResponse>("pause_workflow_run", {
    workflowRunId,
    overrideReason: override?.reason ?? null,
    idempotencyKey: override?.idempotencyKey ?? null,
  });
}

export async function resumeWorkflowRun(
  workflowRunId: string,
  override?: ManagedRunOverrideOptions,
) {
  return command<TaskResponse>("resume_workflow_run", {
    workflowRunId,
    overrideReason: override?.reason ?? null,
    idempotencyKey: override?.idempotencyKey ?? null,
  });
}

export async function replayWorkflowRun(
  workflowRunId: string,
  options: { fromStepId?: string; override?: ManagedRunOverrideOptions } = {},
) {
  return command<WorkflowRunCreated>("replay_workflow_run", {
    workflowRunId,
    fromStepId: options.fromStepId ?? null,
    overrideReason: options.override?.reason ?? null,
    idempotencyKey: options.override?.idempotencyKey ?? null,
  });
}

export async function renameWorkflowRun(workflowRunId: string, name: string | null) {
  return command<TaskResponse>("rename_workflow_run", { workflowRunId, name });
}

export interface NotificationListOptions {
  unreadOnly?: boolean;
  limit?: number;
}

export async function fetchNotifications(options: NotificationListOptions = {}) {
  return command<Notification[]>("fetch_notifications", {
    unreadOnly: Boolean(options.unreadOnly),
    limit: options.limit ?? 200,
  });
}

export async function markNotificationRead(notificationId: string) {
  return command<Notification>("mark_notification_read", { notificationId });
}

export async function markAllNotificationsRead() {
  return command<TaskResponse>("mark_all_notifications_read");
}

export async function deleteNotification(notificationId: string) {
  return command<TaskResponse>("delete_notification", { notificationId });
}

export async function fetchNotificationDeliveries(notificationId: string) {
  return command<NotificationDelivery[]>("fetch_notification_deliveries", { notificationId });
}

export async function fetchNotificationPolicies(workflowId?: string) {
  return command<NotificationPolicy[]>("fetch_notification_policies", { workflowId });
}

export async function createNotificationPolicy(policy: NewNotificationPolicy) {
  return command<NotificationPolicy>("create_notification_policy", { policy });
}

export async function updateNotificationPolicy(policyId: string, policy: NewNotificationPolicy) {
  return command<NotificationPolicy>("update_notification_policy", { policyId, policy });
}

export async function deleteNotificationPolicy(policyId: string) {
  return command<TaskResponse>("delete_notification_policy", { policyId });
}

export async function fetchFreezeWindows(activeOnly = false) {
  return command<FreezeWindow[]>("fetch_freeze_windows", { activeOnly });
}

export async function createFreezeWindow(window: NewFreezeWindow) {
  return command<FreezeWindow>("create_freeze_window", { window });
}

export async function updateFreezeWindow(windowId: string, window: NewFreezeWindow) {
  return command<FreezeWindow>("update_freeze_window", { windowId, window });
}

export async function deleteFreezeWindow(windowId: string) {
  return command<TaskResponse>("delete_freeze_window", { windowId });
}

export async function backfillWorkflowTrigger(triggerId: string, request: BackfillRequest) {
  return command<BackfillResponse>("backfill_workflow_trigger", { triggerId, request });
}

export async function createCalendarSubscription(scope: CalendarScope, orgId?: string | null) {
  return command<CalendarSubscriptionSecret>("create_calendar_subscription", { scope, orgId });
}

export async function deleteCalendarSubscription(subscriptionId: string): Promise<void> {
  await command<unknown>("delete_calendar_subscription", { subscriptionId });
}

export function calendarSubscriptionUrl(token: string, serviceUrl?: string | null): string {
  const base = (serviceUrl?.trim() ? serviceUrl : apiBaseUrl()).replace(/\/+$/, "");
  const path = `${base}/calendar/${encodeURIComponent(token)}/runinator.ics`;
  return typeof window === "undefined" ? path : new URL(path, window.location.origin).toString();
}

export function downloadScheduleCalendar(scope: CalendarScope, orgId?: string | null) {
  if (isTauriRuntime()) {
    return command<number[]>("download_schedule_calendar", { scope, orgId }).then(
      (bytes) => new Blob([new Uint8Array(bytes)], { type: "text/calendar;charset=utf-8" }),
    );
  }

  const query = new URLSearchParams({ scope });

  if (orgId) {
    query.set("org_id", orgId);
  }

  return downloadBinary(`schedules/calendar.ics?${query.toString()}`);
}

export async function deleteArtifact(artifactId: string) {
  return command<TaskResponse>("delete_artifact", { artifactId });
}

export async function deleteGate(gateId: string) {
  return command<TaskResponse>("delete_gate", { gateId });
}

export async function deleteAutomationEvent(eventId: string) {
  return command<TaskResponse>("delete_automation_event", { eventId });
}

export interface ReplicaSample {
  replica_id: string;
  sampled_at: string;
  cpu_percent: number;
  mem_percent: number;
  mem_used_bytes: number;
  mem_total_bytes: number;
  load_one?: number | null;
  process_cpu_percent: number;
  process_mem_bytes: number;
  net_rx_bytes_per_sec: number;
  net_tx_bytes_per_sec: number;
}

export interface ReplicaSampleSeries {
  replica_id: string;
  samples: ReplicaSample[];
}

export async function fetchReplicaSamples(replicaId: string, sinceSeconds?: number) {
  return command<ReplicaSampleSeries>("fetch_replica_samples", { replicaId, sinceSeconds });
}

export async function fetchReplicaProviders(replicaId: string) {
  return command<ReplicaProviderRegistration[]>("fetch_replica_providers", { replicaId });
}

export async function setWorkflowOwner(workflowId: string, orgId: string | null) {
  await transferResourceOwner(
    "workflow",
    workflowId,
    orgId == null ? "platform" : "organization",
    orgId,
  );

  const workflow = (await fetchWorkflows()).find((candidate) => candidate.id === workflowId);

  if (workflow == null) {
    throw new Error(`Workflow ${workflowId} was not returned after its ownership transfer`);
  }

  return workflow;
}

export interface SupervisorProcessSnapshot {
  name: string;
  status: string;
  pid?: number | null;
  restarts: number;
  uptime_seconds?: number | null;
  last_exit_code?: number | null;
  last_error?: string | null;
  started_at?: string | null;
  command: string;
  cwd: string;
  log_file: string;
}

export interface SupervisorStatus {
  configured: boolean;
  path?: string;
  supervisor_pid?: number;
  config_path?: string;
  started_at?: string;
  updated_at?: string;
  processes?: SupervisorProcessSnapshot[];
  stale_seconds?: number | null;
  error?: string;
}

export async function fetchSupervisorStatus() {
  return command<SupervisorStatus>("fetch_supervisor_status");
}

export async function fetchResourceRecords(endpoint: string) {
  return command<JsonRecord[]>("fetch_resource_records", { endpoint });
}

export async function fetchProviders() {
  return command<ProviderMetadata[]>("fetch_providers");
}

export async function fetchNodeKinds() {
  return command<WorkflowNodeKindMetadata[]>("fetch_node_kinds");
}

export async function fetchTriggerKinds() {
  return command<WorkflowTriggerKindMetadata[]>("fetch_trigger_kinds");
}

export async function fetchEnumCatalogs() {
  return command<EnumCatalogMetadata[]>("fetch_enum_catalogs");
}

export async function fetchReplicas() {
  return command<ReplicaListResponse>("fetch_replicas");
}

export async function createAgentDirective(replicaId: string, kind: AgentDirectiveKind) {
  return command<AgentDirectiveRecord>("create_agent_directive", { replicaId, kind });
}

export async function listAgentDirectives(replicaId: string, limit = 50) {
  return command<AgentDirectiveRecord[]>("list_agent_directives", { replicaId, limit });
}

export async function createAgentEnrollmentToken(request: CreateAgentEnrollmentTokenInput) {
  return command<CreateAgentEnrollmentTokenResponse>("create_agent_enrollment_token", { request });
}

export async function listAgentEnrollmentTokens() {
  return command<AgentEnrollmentToken[]>("list_agent_enrollment_tokens");
}

export async function revokeAgentEnrollmentToken(tokenId: string) {
  return command<TaskResponse>("revoke_agent_enrollment_token", { tokenId });
}

export async function listAgentMachines() {
  return command<AgentMachineEnrollment[]>("list_agent_machines");
}

export async function invalidateAgentMachine(machineId: string) {
  return command<TaskResponse>("invalidate_agent_machine", { machineId });
}

export async function kickReplica(replicaId: string) {
  return command<ReplicaRecord>("kick_replica", { replicaId });
}

// --- on-demand node provisioning (supervisor / kubernetes backends) ---

export interface NodeBackendInfo {
  backend: string;
  kinds: string[];
  available: boolean;
}

export interface NodeBackendsResponse {
  backends: NodeBackendInfo[];
}

export interface ProvisionedGroup {
  backend: string;
  kind: string;
  name: string;
  desired: number;
  available: number;
  manageable: boolean;
  // smallest desired count the backend allows (a floor of one for control-plane kinds).
  min_desired?: number;
}

export interface NodeSpec {
  labels?: Record<string, string>;
  image?: string | null;
  extra_args?: string[];
  group?: string | null;
}

export interface ScaleNodesRequest {
  backend: string;
  kind: string;
  desired: number;
  spec?: NodeSpec;
}

export interface StopNodeRequest {
  backend: string;
  node_id: string;
}

export async function fetchNodeBackends() {
  return command<NodeBackendsResponse>("fetch_node_backends");
}

export async function fetchNodes() {
  return command<ProvisionedGroup[]>("fetch_nodes");
}

export async function scaleNodes(request: ScaleNodesRequest) {
  return command<ProvisionedGroup>("scale_nodes", { request });
}

export async function stopNode(request: StopNodeRequest) {
  return command<JsonRecord>("stop_node", { request });
}

// --- organizations (tenants), membership, resource allocation, and billing ---

export type OrgRole = "owner" | "admin" | "member";

export interface Organization {
  id: string;
  name: string;
  slug: string;
  disabled: boolean;
  created_at: string;
  updated_at: string;
}

export interface OrgMembershipView {
  org: Organization;
  role: OrgRole;
}

export interface OrgMembership {
  org_id: string;
  user_id: string;
  role: OrgRole;
  created_at: string;
}

export interface OrgContextResponse {
  access_token: string;
  expires_in: number;
  org: Organization;
  role: OrgRole;
}

export interface OrgResourceGroup {
  org_id: string;
  backend: string;
  kind: string;
  desired: number;
  dedicated: boolean;
}

export interface OrgNodesResponse {
  groups: OrgResourceGroup[];
  projected_monthly_cents: number;
}

export interface OrgQuota {
  org_id: string;
  max_nodes_per_kind: Record<string, number>;
  max_monthly_cents: number;
}

export interface OrgUsage {
  org_id: string;
  since: string | null;
  node_hours: Record<string, number>;
  accrued_cents: number;
}

export interface RateEntry {
  backend: string;
  kind: string;
  hourly_cents: number;
}

export interface RateCard {
  entries: RateEntry[];
}

export interface ScaleOrgNodesRequest {
  backend: string;
  kind: string;
  desired: number;
}

export async function listMyOrgs() {
  return command<OrgMembershipView[]>("list_my_orgs");
}

export async function listOrgs() {
  return command<Organization[]>("list_orgs");
}

export async function createOrg(name: string) {
  return command<Organization>("create_org", { name });
}

export async function switchOrg(orgId: string) {
  return command<OrgContextResponse>("switch_org", { orgId });
}

export async function listOrgMembers(orgId: string) {
  return command<OrgMembership[]>("list_org_members", { orgId });
}

export async function addOrgMember(orgId: string, userId: string, role: OrgRole) {
  return command<JsonRecord>("add_org_member", { orgId, userId, role });
}

export async function updateOrgMember(orgId: string, userId: string, role: OrgRole) {
  return command<JsonRecord>("update_org_member", { orgId, userId, role });
}

export async function removeOrgMember(orgId: string, userId: string) {
  return command<JsonRecord>("remove_org_member", { orgId, userId });
}

export async function fetchRateCard() {
  return command<RateCard>("fetch_rate_card");
}

export async function fetchOrgNodes(orgId: string) {
  return command<OrgNodesResponse>("fetch_org_nodes", { orgId });
}

export async function scaleOrgNodes(orgId: string, request: ScaleOrgNodesRequest) {
  return command<OrgResourceGroup>("scale_org_nodes", { orgId, request });
}

export async function fetchOrgQuota(orgId: string) {
  return command<OrgQuota>("fetch_org_quota", { orgId });
}

export async function fetchOrgUsage(orgId: string) {
  return command<OrgUsage>("fetch_org_usage", { orgId });
}

export async function fetchCredentials() {
  return command<CredentialSummary[]>("fetch_credentials");
}

export async function fetchCredential(scope: string, name: string, kind: SettingKind = "secret") {
  return command<CredentialDetail>("fetch_credential", { scope, name, kind });
}

export async function saveCredential(
  scope: string,
  name: string,
  value: unknown,
  kind: SettingKind = "secret",
  schema?: unknown,
  expiresAt?: string | null,
) {
  return command<TaskResponse>("save_credential", {
    request: { scope, name, value, kind, schema, expires_at: expiresAt },
  });
}

export async function moveCredential(
  id: string,
  scope: string,
  name: string,
  kind: SettingKind,
) {
  return command<CredentialSummary>("move_credential", {
    settingId: id,
    request: { scope, name, kind },
  });
}

export async function deleteCredential(scope: string, name: string, kind: SettingKind = "secret") {
  return command<TaskResponse>("delete_credential", { scope, name, kind });
}

export async function fetchForeignLanguageRuntime(language: string) {
  return fetchCredential(FOREIGN_LANGUAGE_SCOPE, language, "config") as Promise<
    CredentialDetail & { value?: ForeignLanguageRuntimeConfig }
  >;
}

export async function saveForeignLanguageRuntime(
  language: string,
  value: ForeignLanguageRuntimeConfig,
) {
  return saveCredential(FOREIGN_LANGUAGE_SCOPE, language, value, "config");
}

// pending approval requests, all of them or one run's.
export async function fetchApprovals(workflowRunId?: string) {
  return command<JsonRecord[]>("fetch_approvals", { workflowRunId: workflowRunId ?? null });
}

export interface ApprovalResolution {
  by?: string | null;
  message?: string | null;
  output?: unknown;
}

export async function approveApproval(approvalId: string, resolution: ApprovalResolution = {}) {
  return settleWorkflowEffect(
    approvalId,
    "succeeded",
    (resolution.output ?? { decision: "approved" }) as JsonValue,
    resolution.message ?? null,
  );
}

export async function rejectApproval(approvalId: string, resolution: ApprovalResolution = {}) {
  // "rejected", not "failed": the graph routes an approval's on_reject edge apart from its
  // on_failure edge, and the terminal effect status is what the VM classifies the failure by.
  return settleWorkflowEffect(
    approvalId,
    "rejected",
    (resolution.output ?? { decision: "rejected" }) as JsonValue,
    resolution.message ?? null,
  );
}

export async function fetchGates(workflowRunId?: string, status?: string) {
  const query = new URLSearchParams();

  if (workflowRunId?.trim()) {
    query.set("workflow_run_id", workflowRunId.trim());
  }

  if (status?.trim()) {
    query.set("status", status.trim());
  }

  const suffix = query.size ? `?${query.toString()}` : "";
  return command<GateRecord[]>("fetch_resource_records", { endpoint: `gates${suffix}` });
}

export async function openGate(gateId: string, reason?: string) {
  return settleWorkflowEffect(gateId, "succeeded", { open: true, reason: reason ?? null }, reason);
}

export async function closeGate(gateId: string, reason?: string) {
  return settleWorkflowEffect(gateId, "succeeded", { open: false, reason: reason ?? null }, reason);
}

export async function deliverSignal(workflowRunId: string, name: string, payload: unknown = {}) {
  return command<TaskResponse>("deliver_signal", { workflowRunId, name, payload });
}

/**
 * ask a run to raise an interrupt.
 *
 * this only *records* the request on the run. the reducer decides whether it can be serviced on the
 * next drive, and every refusal is silent, so a `success` response means "recorded", not "raised".
 */
export async function requestRunInterrupt(
  workflowRunId: string,
  source: string,
  payload: unknown = null,
  continuationId: string | null = null,
) {
  return command<TaskResponse>("request_run_interrupt", {
    workflowRunId,
    source,
    payload,
    continuationId,
  });
}

export interface PackImportResult {
  workflows: WorkflowBundle;
  secrets: { secrets: unknown[] };
  pipelines: Pipeline[];
}

/** Upload a compiled pack ZIP. Source packs are compiled locally before reaching this endpoint. */
export async function importPackArchive(bytes: ArrayBuffer, overwrite = false) {
  return command<PackImportResult>(
    "import_pack_archive",
    isTauriRuntime() ? { base64: base64Encode(bytes), overwrite } : { bytes, overwrite },
  );
}

// ---- VM-native workflow files ----

export async function fetchWorkflowFiles() {
  return command<WorkflowFile[]>("list_workflow_files", {});
}

export async function fetchExecutionProfiles() {
  return command<import("../domain/models").ExecutionProfile[]>("list_execution_profiles", {});
}

export async function putExecutionProfile(
  profileId: string,
  profile: import("../domain/models").ExecutionProfileInput,
) {
  return command<import("../domain/models").ExecutionProfile>("put_execution_profile", {
    profileId,
    profile,
  });
}

export async function deleteExecutionProfile(profileId: string) {
  return command<{ success: boolean }>("delete_execution_profile", { profileId });
}

export async function rotateExecutionProfile(profileId: string) {
  return command<{ success: boolean }>("rotate_execution_profile", { profileId });
}

export async function testExecutionProfile(profileId: string) {
  return command<{ success: boolean }>("test_execution_profile", { profileId });
}

export async function uploadWorkflowFile(path: string, file: File, staged = false) {
  const bytes = await file.arrayBuffer();

  if (isTauriRuntime()) {
    return command<WorkflowFile>(staged ? "stage_workflow_file" : "upload_workflow_file", {
      path,
      mimeType: file.type || "application/octet-stream",
      base64: base64Encode(bytes),
    });
  }

  return command<WorkflowFile>(staged ? "stage_workflow_file" : "upload_workflow_file", {
    path,
    mimeType: file.type || "application/octet-stream",
    bytes,
  });
}

export async function archiveWorkflowFile(fileId: string) {
  return command<{ success: boolean }>("archive_workflow_file", { fileId });
}

async function downloadBinary(path: string): Promise<Blob> {
  const token = httpAuthToken();
  const response = await fetch(`${apiBaseUrl()}/${path.replace(/^\/+/, "")}`, {
    headers: token ? { authorization: `Bearer ${token}` } : undefined,
  });

  if (!response.ok) {
    throw new Error(`Download failed (${String(response.status)})`);
  }

  return response.blob();
}

export function downloadWorkflowFileContent(fileId: string) {
  if (isTauriRuntime()) {
    return command<number[]>("download_workflow_file", { fileId }).then(
      (bytes) => new Blob([new Uint8Array(bytes)]),
    );
  }

  return downloadBinary(`workflow_files/${encodeURIComponent(fileId)}/content`);
}

export function downloadWorkflowEffectArtifact(effectId: string, eventId: string) {
  if (isTauriRuntime()) {
    return command<number[]>("download_workflow_effect_artifact", { effectId, eventId }).then(
      (bytes) => new Blob([new Uint8Array(bytes)]),
    );
  }

  return downloadBinary(
    `workflow_effects/${encodeURIComponent(effectId)}/output/${encodeURIComponent(eventId)}/artifact`,
  );
}

// ---- packaged functions ----

export async function fetchFunctionPackages() {
  return command<FunctionPackage[]>("list_function_packages", {});
}

export async function fetchFunctionPackage(packageName: string) {
  return command<FunctionPackageDetail>("fetch_function_package", { packageName });
}

// every published export, including older versions: a workflow pinned to version 2 still needs to be
// explicable after version 3 ships.
export async function fetchFunctionCatalog() {
  return command<FunctionCatalogEntry[]>("fetch_function_catalog", {});
}

export async function deleteFunctionPackage(packageName: string) {
  return command<JsonRecord>("delete_function_package", { packageName });
}

export async function restoreFunctionPackage(packageName: string) {
  return command<JsonRecord>("restore_function_package", { packageName });
}

/// store a package archive under the digest of its bytes.
///
/// the server keeps it only if it does not already hold that digest, so re-publishing unchanged
/// code moves the bytes once and never again.
export async function uploadFunctionArtifact(digest: string, bytes: ArrayBuffer) {
  // tauri's ipc is json, so the desktop build hands the archive over base64-encoded and the rust
  // side decodes it; the web build posts the bytes themselves.
  return command<FunctionArtifact>(
    "upload_function_artifact",
    isTauriRuntime() ? { digest, base64: base64Encode(bytes) } : { digest, bytes },
  );
}

function base64Encode(bytes: ArrayBuffer): string {
  // chunked so a multi-megabyte archive does not blow the argument limit of `String.fromCharCode`.
  const view = new Uint8Array(bytes);
  const chunks: string[] = [];

  for (let at = 0; at < view.length; at += 0x8000) {
    chunks.push(String.fromCharCode(...view.subarray(at, at + 0x8000)));
  }

  return btoa(chunks.join(""));
}

/// publish one version of a package against an artifact already stored.
export async function publishFunctionVersion(request: NewFunctionVersion) {
  return command<FunctionVersion>("publish_function_version", { request });
}

// point an alias at a version, by number or at whatever another alias currently names.
export async function setFunctionAlias(
  packageName: string,
  alias: string,
  version?: number,
  fromAlias?: string,
) {
  return command<JsonRecord>("set_function_alias", {
    packageName,
    alias,
    version: version ?? null,
    fromAlias: fromAlias ?? null,
  });
}

export async function deleteFunctionAlias(packageName: string, alias: string) {
  return command<JsonRecord>("delete_function_alias", { packageName, alias });
}

export async function invokeFunction(
  packageName: string,
  exportName: string,
  input: JsonRecord,
  // an alias resolves at call time and a version pins; passing neither takes whatever the package's
  // default alias currently names.
  selector: { alias?: string | null; version?: number | null } = {},
) {
  return command<JsonRecord>("invoke_function", {
    packageName,
    exportName,
    input,
    alias: selector.alias ?? null,
    version: selector.version ?? null,
  });
}

// ---- the rexrap console ----

export async function fetchConsoleSessions() {
  return command<ConsoleSession[]>("list_console_sessions", {});
}

export async function createConsoleSession(name?: string) {
  return command<ConsoleSession>("create_console_session", { name: name ?? null });
}

export async function fetchConsoleSession(sessionId: string) {
  return command<ConsoleSessionDetail>("fetch_console_session", { sessionId });
}

export async function renameConsoleSession(sessionId: string, name: string) {
  return command<ConsoleSessionDetail>("rename_console_session", { sessionId, name });
}

export async function deleteConsoleSession(sessionId: string) {
  return command<JsonRecord>("delete_console_session", { sessionId });
}

export async function clearConsoleSession(sessionId: string) {
  return command<JsonRecord>("clear_console_session", { sessionId });
}

export async function createConsoleCell(sessionId: string, source: string, label?: string | null) {
  return command<ConsoleCell>("create_console_cell", {
    sessionId,
    source,
    label: label ?? null,
    position: null,
  });
}

// Re-read a cell to follow a scratch run. The backend settles the cell from
// the run before answering, so a poll never shows `running` forever.
export async function fetchConsoleCell(cellId: string) {
  return command<ConsoleCell>("fetch_console_cell", { cellId });
}

export async function updateConsoleCell(cellId: string, source: string, label?: string | null) {
  return command<ConsoleCell>("update_console_cell", {
    cellId,
    source,
    label: label ?? null,
    position: null,
  });
}

export async function deleteConsoleCell(cellId: string) {
  return command<JsonRecord>("delete_console_cell", { cellId });
}

export async function runConsoleCell(cellId: string) {
  return command<ConsoleCell>("run_console_cell", { cellId });
}

export async function cancelConsoleCell(cellId: string) {
  return command<JsonRecord>("cancel_console_cell", { cellId });
}

export async function replayConsoleCell(cellId: string) {
  return command<ConsoleCell>("replay_console_cell", { cellId });
}
