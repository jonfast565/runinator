import { command, isTauriRuntime } from "./runtime";
import { setHttpAuthToken } from "./httpRuntime";
import { asJsonRecord } from "../domain/json";
import type {
  JsonRecord,
  JsonValue,
  ApiKey,
  AgentEnrollmentToken,
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
  RunArtifact,
  RunChunk,
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
} from "../domain/models";

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

export async function revokeResourceGrant(resourceType: string, resourceId: string, grantId: string) {
  return command<TaskResponse>("revoke_resource_grant", { resourceType, resourceId, grantId });
}

export async function transferResourceOwner(
  resourceType: "workflow" | "pipeline" | "function_package" | "console_session",
  resourceId: string,
  scopeKind: "platform" | "organization" | "team" | "user",
  scopeId: string | null,
) {
  return command<JsonRecord>("transfer_resource_owner", { resourceType, resourceId, scopeKind, scopeId });
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

export async function addTeamMember(teamId: string, userId: string, role: "owner" | "admin" | "operator" | "member") {
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

export async function fetchRunChunks(runId: string) {
  return command<RunChunk[]>("fetch_run_chunks", { runId });
}

export async function fetchRunArtifacts(runId: string) {
  return command<RunArtifact[]>("fetch_run_artifacts", { runId });
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
      return [{
        id: event.event_id,
        workflow_run_id: event.workflow_run_id,
        // VM effects, rather than node-run rows, own this output. The effect id is the durable
        // execution identity that lets an operator correlate the artifact to the VM debugger.
        node_id: event.effect_id,
        // A VM artifact is addressed by its effect-output event and URI; legacy run_artifact ids
        // no longer exist, so the old `/artifacts/{id}/download` endpoint cannot serve it.
        artifact_id: null,
        name: typeof artifact.name === "string" ? artifact.name : "artifact",
        mime_type: typeof artifact.mime_type === "string" ? artifact.mime_type : "application/octet-stream",
        size_bytes: typeof artifact.size_bytes === "number" ? artifact.size_bytes : 0,
        uri: typeof artifact.uri === "string" ? artifact.uri : "",
        metadata: asJsonRecord(artifact.metadata),
        created_at: new Date(event.created_at * 1000).toISOString(),
      } satisfies WorkflowRunArtifact];
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

// the edges a single run actually walked, in order, reconstructed from the node-run chain.
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
  await transferResourceOwner("pipeline", pipelineId, orgId == null ? "platform" : "organization", orgId);
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

export async function fetchPipelineRun(pipelineRunId: string) {
  return command<PipelineRunDetail>("fetch_pipeline_run", { pipelineRunId });
}

export async function deletePipelineRun(pipelineRunId: string) {
  return command<TaskResponse>("delete_pipeline_run", { pipelineRunId });
}

export async function cancelPipelineRun(pipelineRunId: string) {
  return command<TaskResponse>("cancel_pipeline_run", { pipelineRunId });
}

export async function pausePipelineRun(pipelineRunId: string) {
  return command<TaskResponse>("pause_pipeline_run", { pipelineRunId });
}

export async function resumePipelineRun(pipelineRunId: string) {
  return command<TaskResponse>("resume_pipeline_run", { pipelineRunId });
}

export async function retryPipelineMember(
  pipelineRunId: string,
  memberKey: string,
  parameters: unknown = {},
) {
  return command<PipelineMemberAttempt>("retry_pipeline_member", {
    pipelineRunId,
    memberKey,
    parameters,
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
  options: { debug?: boolean; parameters?: unknown } = {},
) {
  return command<WorkflowRunCreated>("create_workflow_run", {
    workflowId,
    debug: Boolean(options.debug),
    parameters: options.parameters ?? {},
  });
}

export async function fetchWorkflowRuns(workflowId?: string) {
  return command<RunSummary[]>("fetch_workflow_runs", { workflowId });
}

export async function fetchWorkflowRun(workflowRunId: string): Promise<WorkflowRunDetail> {
  const [detail, continuations, effects, journal, vmCursors] = await Promise.all([
    command<WorkflowRunDetail>("fetch_workflow_run", { workflowRunId }),
    fetchWorkflowContinuations(workflowRunId),
    fetchWorkflowEffects(workflowRunId),
    fetchWorkflowJournal(workflowRunId),
    fetchWorkflowVmCursors(workflowRunId),
  ]);
  const cursorByContinuation = new Map(
    vmCursors.map((cursor) => [cursor.continuation_id, cursor] as const),
  );
  const nodes: WorkflowNodeRun[] = effects.flatMap((effect) => {
    const cursor = cursorByContinuation.get(effect.continuation_id);

    // VM effects retain their originating graph node through the journal projection. A live
    // cursor is only a fallback for older servers; it moves after an effect settles and cannot
    // describe the historical node that ran.
    const nodeId = effect.node_id ?? cursor?.node_id;

    if (!nodeId) {
      return [];
    }

    const request =
      typeof effect.request === "object" && effect.request !== null
        ? (effect.request as JsonRecord)
        : {};
    const requestType = typeof request.type === "string" ? request.type : "";
    const status =
      effect.status === "requested" || effect.status === "running"
        ? requestType === "approval"
          ? "approval_required"
          : ["input", "signal", "gate", "event_wait"].includes(requestType)
            ? "waiting"
            : effect.status
        : effect.status;
    return [{
      id: effect.id,
      workflow_run_id: effect.workflow_run_id,
      node_id: nodeId,
      status,
      attempt: effect.attempt,
      parameters: request,
      output_json: effect.result ?? null,
      state: { effect_id: effect.id, ...request },
      cursor_id: effect.continuation_id,
      created_at: new Date(effect.created_at * 1000).toISOString(),
      started_at: new Date(effect.created_at * 1000).toISOString(),
      finished_at: effect.finished_at
        ? new Date(effect.finished_at * 1000).toISOString()
        : null,
      message: effect.message ?? null,
    }];
  });
  return {
    ...detail,
    // Temporary graph-view projection. These are derived entirely from VM records and do not
    // fetch or mutate the legacy node-run resource.
    nodes,
    continuations,
    effects,
    journal,
    vm_cursors: vmCursors,
    execution_state: {
      ...(detail.execution_state ?? {}),
      cursors: vmCursors.map((cursor) => ({
        id: cursor.continuation_id,
        node_id: cursor.node_id ?? "",
        debug: { paused: cursor.status === "paused" },
      })).filter((cursor) => cursor.node_id.length > 0),
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

export async function cancelWorkflowRun(workflowRunId: string) {
  return command<TaskResponse>("cancel_workflow_run", { workflowRunId });
}

export async function pauseWorkflowRun(workflowRunId: string) {
  return command<TaskResponse>("pause_workflow_run", { workflowRunId });
}

export async function resumeWorkflowRun(workflowRunId: string) {
  return command<TaskResponse>("resume_workflow_run", { workflowRunId });
}

export async function replayWorkflowRun(
  workflowRunId: string,
  options: { fromStepId?: string } = {},
) {
  return command<WorkflowRunCreated>("replay_workflow_run", {
    workflowRunId,
    fromStepId: options.fromStepId ?? null,
  });
}

export async function renameWorkflowRun(workflowRunId: string, name: string | null) {
  return command<TaskResponse>("rename_workflow_run", { workflowRunId, name });
}

export interface ArtifactUploadRequest {
  run_id: string;
  workflow_node_run_id?: string | null;
}

export interface ArtifactDownloadResult {
  saved_to: string | null;
}

export async function fetchAllArtifacts() {
  return command<RunArtifact[]>("fetch_all_artifacts");
}

export async function uploadArtifactFromPath(request: ArtifactUploadRequest) {
  return command<RunArtifact>("upload_artifact", { request });
}

export async function downloadArtifactToPath(artifactId: string, defaultName: string) {
  return command<ArtifactDownloadResult>("download_artifact", { artifactId, defaultName });
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
  await transferResourceOwner("workflow", workflowId, orgId == null ? "platform" : "organization", orgId);

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
) {
  return command<TaskResponse>("save_credential", {
    request: { scope, name, value, kind, schema },
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
  cursorId: string | null = null,
) {
  return command<TaskResponse>("request_run_interrupt", {
    workflowRunId,
    source,
    payload,
    cursorId,
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
