import { invoke } from "@tauri-apps/api/core";
import { isTauriRuntime } from "./tauriRuntime";
import { apiBaseUrl, invokeViaHttp, setHttpAuthToken } from "./httpRuntime";
import type {
  JsonRecord,
  ApiKey,
  CreateApiKeyResponse,
  CredentialSummary,
  CredentialDetail,
  DevPackApplyResult,
  DevPackInspectResult,
  DevPackTextFile,
  GateRecord,
  Grant,
  Notification,
  PermissionLevel,
  PrincipalType,
  ProviderMetadata,
  ReplicaListResponse,
  RunArtifact,
  RunChunk,
  RunSummary,
  SaveTaskResponse,
  SettingKind,
  ScheduledTask,
  ServiceStatus,
  TaskResponse,
  Team,
  User,
  WdlCompletionRequest,
  WdlCompletionResponse,
  WdlDiagnostic,
  WdlHoverRequest,
  WdlHoverResponse,
  WorkflowBundle,
  WorkflowDefinition,
  WorkflowRunCreated,
  WorkflowRunArtifact,
  WorkflowRunDetail,
  WorkflowTrigger
} from "../types/models";

export interface WorkflowWdlSaveRequest {
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

function command<T>(name: string, args?: Record<string, unknown>) {
  if (isTauriRuntime()) return invoke<T>(name, args);
  return invokeViaHttp<T>(name, args);
}

export interface AuthConfigResponse {
  enabled: boolean;
}

export interface LoginResult {
  access_token: string;
  refresh_token: string;
  expires_in: number;
  user: JsonRecord;
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

// push the access token to both runtimes: the web fetch layer and (on desktop) the tauri client.
export async function setAccessToken(token: string | null) {
  setHttpAuthToken(token);
  if (isTauriRuntime()) await command<void>("set_access_token", { token });
}

export async function listWorkflowGrants(workflowId: string) {
  return command<JsonRecord[]>("list_workflow_grants", { workflowId });
}

export async function createWorkflowGrant(
  workflowId: string,
  principalType: "user" | "team",
  principalId: string,
  permission: "view" | "run" | "edit" | "own"
) {
  return command<JsonRecord>("create_workflow_grant", {
    workflowId,
    principalType,
    principalId,
    permission
  });
}

export async function revokeWorkflowGrant(workflowId: string, grantId: string) {
  return command<any>("revoke_workflow_grant", { workflowId, grantId });
}

export interface CreateUserInput {
  username: string;
  password: string;
  email?: string | null;
  is_admin?: boolean;
}

export interface UpdateUserInput {
  email?: string | null;
  password?: string | null;
  is_admin?: boolean | null;
  disabled?: boolean | null;
}

export interface CreateApiKeyInput {
  name: string;
  user_id?: string | null;
  is_service?: boolean;
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

export async function addTeamMember(teamId: string, userId: string) {
  return command<TaskResponse>("add_team_member", { teamId, userId });
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

export async function grantWorkflowAccess(
  workflowId: string,
  principalType: PrincipalType,
  principalId: string,
  permission: PermissionLevel
) {
  return command<Grant>("create_workflow_grant", {
    workflowId,
    principalType,
    principalId,
    permission
  });
}

export async function getServiceStatus() {
  return command<ServiceStatus>("get_service_status");
}

export async function startServiceDiscovery() {
  return command("start_service_discovery");
}

export async function fetchTasks() {
  return command<ScheduledTask[]>("fetch_tasks");
}

export async function saveTask(task: ScheduledTask, creating: boolean) {
  return command<SaveTaskResponse>("save_task", { request: { task, creating } });
}

export async function deleteTask(taskId: string) {
  return command<TaskResponse>("delete_task", { taskId });
}

export async function requestTaskRun(taskId: string) {
  return command<any>("request_task_run", { taskId });
}

export async function fetchTaskRuns(taskId: string) {
  return command<RunSummary[]>("fetch_task_runs", { taskId });
}

export async function fetchRunChunks(runId: string) {
  return command<RunChunk[]>("fetch_run_chunks", { runId });
}

export async function fetchRunArtifacts(runId: string) {
  return command<RunArtifact[]>("fetch_run_artifacts", { runId });
}

export async function fetchWorkflowNodeRunChunks(nodeRunId: string) {
  return command<RunChunk[]>("fetch_workflow_node_run_chunks", { nodeRunId });
}

export async function fetchWorkflowNodeRunArtifacts(nodeRunId: string) {
  return command<RunArtifact[]>("fetch_workflow_node_run_artifacts", { nodeRunId });
}

export async function fetchWorkflowRunArtifacts(workflowRunId: string) {
  return command<WorkflowRunArtifact[]>("fetch_workflow_run_artifacts", { workflowRunId });
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

export async function saveWorkflowWdl(request: WorkflowWdlSaveRequest) {
  return command<WorkflowBundle>("save_workflow_wdl", { request });
}

export async function compileWdl(source: string, enabled: boolean) {
  return command<WorkflowDefinition>("compile_wdl", { source, enabled });
}

export async function analyzeWdl(source: string, sourcePath?: string | null) {
  return command<WdlDiagnostic[]>("analyze_wdl", { source, sourcePath: sourcePath ?? null });
}

export async function completeWdl(request: WdlCompletionRequest) {
  return command<WdlCompletionResponse>("complete_wdl", { request });
}

export async function hoverWdl(request: WdlHoverRequest) {
  return command<WdlHoverResponse | null>("hover_wdl", { request });
}

export async function formatWdl(source: string) {
  return command<string>("format_wdl", { source });
}

export async function decompileToWdl(workflow: WorkflowDefinition) {
  return command<string>("decompile_to_wdl", { workflow });
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

export async function duplicateWorkflow(workflowId: string, bump: "major" | "minor" | "patch" = "minor") {
  return command<WorkflowDefinition>("duplicate_workflow", { workflowId, bump });
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

export async function createWorkflowRun(workflowId: string, options: { debug?: boolean; parameters?: unknown } = {}) {
  return command<WorkflowRunCreated>("create_workflow_run", {
    workflowId,
    debug: Boolean(options.debug),
    parameters: options.parameters ?? {}
  });
}

export async function fetchWorkflowRuns(workflowId?: string) {
  return command<RunSummary[]>("fetch_workflow_runs", { workflowId });
}

export async function fetchWorkflowRun(workflowRunId: string) {
  return command<WorkflowRunDetail>("fetch_workflow_run", { workflowRunId });
}

export async function stepWorkflowRun(workflowRunId: string) {
  return command<TaskResponse>("step_workflow_run", { workflowRunId });
}

export async function continueWorkflowRun(workflowRunId: string) {
  return command<TaskResponse>("continue_workflow_run", { workflowRunId });
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

export type WorkflowDebugPatch = {
  breakpoints?: string[];
  mode?: "step_all" | "breakpoints";
  one_shot_breakpoint?: string | null;
};

export async function patchWorkflowRunDebug(workflowRunId: string, patch: WorkflowDebugPatch) {
  return command<TaskResponse>("patch_workflow_run_debug", { workflowRunId, patch });
}

export async function runToCursorWorkflowRun(workflowRunId: string, nodeId: string) {
  return command<TaskResponse>("run_to_cursor_workflow_run", { workflowRunId, nodeId });
}

export async function skipWorkflowNode(workflowRunId: string, outputJson: any, message?: string) {
  return command<TaskResponse>("skip_workflow_node", { workflowRunId, outputJson, message });
}

export async function resolveWorkflowInput(nodeRunId: string, outputJson: any, resolvedBy?: string, message?: string) {
  return command<TaskResponse>("resolve_workflow_input", { nodeRunId, outputJson, resolvedBy, message });
}

export async function rerunWorkflowNode(workflowRunId: string, parameters: any) {
  return command<TaskResponse>("rerun_workflow_node", { workflowRunId, parameters });
}

export async function replayWorkflowRun(workflowRunId: string, options: { fromStepId?: string } = {}) {
  return command<WorkflowRunCreated>("replay_workflow_run", { workflowRunId, fromStepId: options.fromStepId ?? null });
}

export async function renameWorkflowRun(workflowRunId: string, name: string | null) {
  return command<TaskResponse>("rename_workflow_run", { workflowRunId, name });
}

export type ArtifactUploadRequest = {
  run_id: string;
  workflow_node_run_id?: string | null;
};

export type ArtifactDownloadResult = { saved_to: string | null };

export async function fetchAllArtifacts() {
  return command<RunArtifact[]>("fetch_all_artifacts");
}

export async function uploadArtifactFromPath(request: ArtifactUploadRequest) {
  return command<RunArtifact>("upload_artifact", { request });
}

export async function downloadArtifactToPath(artifactId: string, defaultName: string) {
  return command<ArtifactDownloadResult>("download_artifact", { artifactId, defaultName });
}

export async function uploadArtifactFromBrowser(request: ArtifactUploadRequest, file: File) {
  const form = new FormData();
  form.set("run_id", String(request.run_id));
  form.set("name", file.name);
  form.set("mime_type", file.type || "application/octet-stream");
  if (request.workflow_node_run_id != null) {
    form.set("workflow_node_run_id", String(request.workflow_node_run_id));
  }
  form.set("file", file, file.name);
  const response = await fetch(`${apiBaseUrl()}/artifacts/upload`, { method: "POST", body: form });
  if (!response.ok) {
    const text = await response.text().catch(() => "");
    throw new Error(`POST artifacts/upload -> ${response.status}: ${text}`);
  }
  return (await response.json()) as RunArtifact;
}

export async function downloadArtifactInBrowser(artifactId: string, defaultName: string) {
  const response = await fetch(`${apiBaseUrl()}/artifacts/${artifactId}/download`);
  if (!response.ok) {
    const text = await response.text().catch(() => "");
    throw new Error(`GET artifacts/${artifactId}/download -> ${response.status}: ${text}`);
  }
  const blob = await response.blob();
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = defaultName;
  document.body.appendChild(anchor);
  anchor.click();
  anchor.remove();
  URL.revokeObjectURL(url);
}

// trigger a client-side download of an in-memory blob (used to save exported workflow files).
export function downloadBlob(fileName: string, blob: Blob) {
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = fileName;
  document.body.appendChild(anchor);
  anchor.click();
  anchor.remove();
  URL.revokeObjectURL(url);
}

// trigger a client-side download of in-memory text (used to save exported WDL to disk).
export function downloadTextFile(fileName: string, contents: string, mimeType = "text/plain") {
  downloadBlob(fileName, new Blob([contents], { type: mimeType }));
}

export function pickFileFromBrowser(): Promise<File | null> {
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.style.display = "none";
    document.body.appendChild(input);
    let settled = false;
    input.addEventListener("change", () => {
      settled = true;
      const file = input.files && input.files[0] ? input.files[0] : null;
      input.remove();
      resolve(file);
    });
    // when the dialog is canceled there is no change event; clean up on focus.
    window.addEventListener("focus", function onFocus() {
      window.removeEventListener("focus", onFocus);
      setTimeout(() => {
        if (settled) return;
        input.remove();
        resolve(null);
      }, 250);
    });
    input.click();
  });
}

export type NotificationListOptions = { unreadOnly?: boolean; limit?: number };

export async function fetchNotifications(options: NotificationListOptions = {}) {
  return command<Notification[]>("fetch_notifications", {
    unreadOnly: Boolean(options.unreadOnly),
    limit: options.limit ?? 200
  });
}

export async function markNotificationRead(notificationId: string) {
  return command<Notification>("mark_notification_read", { notificationId });
}

export async function markAllNotificationsRead() {
  return command<TaskResponse>("mark_all_notifications_read");
}

export type SupervisorProcessSnapshot = {
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
};

export type SupervisorStatus = {
  configured: boolean;
  path?: string;
  supervisor_pid?: number;
  config_path?: string;
  started_at?: string;
  updated_at?: string;
  processes?: SupervisorProcessSnapshot[];
  stale_seconds?: number | null;
  error?: string;
};

export async function fetchSupervisorStatus() {
  return command<SupervisorStatus>("fetch_supervisor_status");
}

export async function fetchResourceRecords(endpoint: string) {
  return command<JsonRecord[]>("fetch_resource_records", { endpoint });
}

export async function fetchProviders() {
  return command<ProviderMetadata[]>("fetch_providers");
}

export async function fetchReplicas() {
  return command<ReplicaListResponse>("fetch_replicas");
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

// --- embedded desktop worker (tauri runtime only) ---

export interface LocalWorkerStatus {
  running: boolean;
  replica_id: string | null;
  root: string | null;
  broker_url: string | null;
}

export interface LocalWorkerConfig {
  broker_url: string;
  sandbox_root: string;
  allow_write?: boolean;
  user_id?: string | null;
}

export async function localWorkerStatus() {
  return command<LocalWorkerStatus>("local_worker_status");
}

export async function startLocalWorker(config: LocalWorkerConfig) {
  return command<LocalWorkerStatus>("start_local_worker", { config });
}

export async function stopLocalWorker() {
  return command<LocalWorkerStatus>("stop_local_worker");
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
  schema?: unknown
) {
  return command<any>("save_credential", { request: { scope, name, value, kind, schema } });
}

export async function deleteCredential(scope: string, name: string, kind: SettingKind = "secret") {
  return command<any>("delete_credential", { scope, name, kind });
}

export async function fetchForeignLanguageRuntime(language: string) {
  return fetchCredential(FOREIGN_LANGUAGE_SCOPE, language, "config") as Promise<CredentialDetail & { value?: ForeignLanguageRuntimeConfig }>;
}

export async function saveForeignLanguageRuntime(language: string, value: ForeignLanguageRuntimeConfig) {
  return saveCredential(FOREIGN_LANGUAGE_SCOPE, language, value, "config");
}

export async function approveApproval(approvalId: string) {
  return command<any>("approve_approval", { approvalId });
}

export async function rejectApproval(approvalId: string) {
  return command<any>("reject_approval", { approvalId });
}

export async function fetchGates(workflowRunId?: string, status?: string) {
  const query = new URLSearchParams();
  if (workflowRunId?.trim()) query.set("workflow_run_id", workflowRunId.trim());
  if (status?.trim()) query.set("status", status.trim());
  const suffix = query.size ? `?${query.toString()}` : "";
  return command<GateRecord[]>("fetch_resource_records", { endpoint: `gates${suffix}` });
}

export async function openGate(gateId: string, reason?: string) {
  return command<any>("open_gate", { gateId, reason: reason ?? null });
}

export async function closeGate(gateId: string, reason?: string) {
  return command<any>("close_gate", { gateId, reason: reason ?? null });
}

export async function deliverSignal(workflowRunId: string, name: string, payload: unknown = {}) {
  return command<any>("deliver_signal", { workflowRunId, name, payload });
}
