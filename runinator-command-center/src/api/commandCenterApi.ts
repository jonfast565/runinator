import { invoke } from "@tauri-apps/api/core";
import { isTauriRuntime } from "./tauriRuntime";
import { apiBaseUrl, invokeViaHttp } from "./httpRuntime";
import type {
  JsonRecord,
  CredentialSummary,
  DevPackApplyResult,
  DevPackInspectResult,
  DevPackTextFile,
  Notification,
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
  WdlCompletionRequest,
  WdlCompletionResponse,
  WdlDiagnostic,
  WorkflowBundle,
  WorkflowDefinition,
  WorkflowRunCreated,
  WorkflowRunDetail,
  WorkflowTrigger
} from "../types/models";

export interface WorkflowWdlSaveRequest {
  source: string;
  enabled: boolean;
  workflow_id?: number | null;
  triggers?: WorkflowTrigger[];
}

function command<T>(name: string, args?: Record<string, unknown>) {
  if (isTauriRuntime()) return invoke<T>(name, args);
  return invokeViaHttp<T>(name, args);
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

export async function deleteTask(taskId: number) {
  return command<TaskResponse>("delete_task", { taskId });
}

export async function requestTaskRun(taskId: number) {
  return command<any>("request_task_run", { taskId });
}

export async function fetchTaskRuns(taskId: number) {
  return command<RunSummary[]>("fetch_task_runs", { taskId });
}

export async function fetchRunChunks(runId: number) {
  return command<RunChunk[]>("fetch_run_chunks", { runId });
}

export async function fetchRunArtifacts(runId: number) {
  return command<RunArtifact[]>("fetch_run_artifacts", { runId });
}

export async function fetchWorkflowNodeRunChunks(nodeRunId: number) {
  return command<RunChunk[]>("fetch_workflow_node_run_chunks", { nodeRunId });
}

export async function fetchWorkflowNodeRunArtifacts(nodeRunId: number) {
  return command<RunArtifact[]>("fetch_workflow_node_run_artifacts", { nodeRunId });
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

export async function analyzeWdl(source: string) {
  return command<WdlDiagnostic[]>("analyze_wdl", { source });
}

export async function completeWdl(request: WdlCompletionRequest) {
  return command<WdlCompletionResponse>("complete_wdl", { request });
}

export async function formatWdl(source: string) {
  return command<string>("format_wdl", { source });
}

export async function decompileToWdl(workflow: WorkflowDefinition) {
  return command<string>("decompile_to_wdl", { workflow });
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

export async function deleteWorkflow(workflowId: number) {
  return command<TaskResponse>("delete_workflow", { workflowId });
}

export async function fetchWorkflowTriggers(workflowId: number) {
  return command<WorkflowTrigger[]>("fetch_workflow_triggers", { workflowId });
}

export async function saveWorkflowTrigger(trigger: WorkflowTrigger, creating: boolean) {
  return command<WorkflowTrigger>("save_workflow_trigger", { trigger, creating });
}

export async function deleteWorkflowTrigger(triggerId: number) {
  return command<TaskResponse>("delete_workflow_trigger", { triggerId });
}

export async function createWorkflowRun(workflowId: number, options: { debug?: boolean; parameters?: unknown } = {}) {
  return command<WorkflowRunCreated>("create_workflow_run", {
    workflowId,
    debug: Boolean(options.debug),
    parameters: options.parameters ?? {}
  });
}

export async function fetchWorkflowRuns(workflowId?: number) {
  return command<RunSummary[]>("fetch_workflow_runs", { workflowId });
}

export async function fetchWorkflowRun(workflowRunId: number) {
  return command<WorkflowRunDetail>("fetch_workflow_run", { workflowRunId });
}

export async function stepWorkflowRun(workflowRunId: number) {
  return command<TaskResponse>("step_workflow_run", { workflowRunId });
}

export async function continueWorkflowRun(workflowRunId: number) {
  return command<TaskResponse>("continue_workflow_run", { workflowRunId });
}

export async function cancelWorkflowRun(workflowRunId: number) {
  return command<TaskResponse>("cancel_workflow_run", { workflowRunId });
}

export async function pauseWorkflowRun(workflowRunId: number) {
  return command<TaskResponse>("pause_workflow_run", { workflowRunId });
}

export async function resumeWorkflowRun(workflowRunId: number) {
  return command<TaskResponse>("resume_workflow_run", { workflowRunId });
}

export type WorkflowDebugPatch = {
  breakpoints?: string[];
  mode?: "step_all" | "breakpoints";
  one_shot_breakpoint?: string | null;
};

export async function patchWorkflowRunDebug(workflowRunId: number, patch: WorkflowDebugPatch) {
  return command<TaskResponse>("patch_workflow_run_debug", { workflowRunId, patch });
}

export async function runToCursorWorkflowRun(workflowRunId: number, nodeId: string) {
  return command<TaskResponse>("run_to_cursor_workflow_run", { workflowRunId, nodeId });
}

export async function skipWorkflowNode(workflowRunId: number, outputJson: any, message?: string) {
  return command<TaskResponse>("skip_workflow_node", { workflowRunId, outputJson, message });
}

export async function rerunWorkflowNode(workflowRunId: number, parameters: any) {
  return command<TaskResponse>("rerun_workflow_node", { workflowRunId, parameters });
}

export async function replayWorkflowRun(workflowRunId: number, options: { fromStepId?: string } = {}) {
  return command<WorkflowRunCreated>("replay_workflow_run", { workflowRunId, fromStepId: options.fromStepId ?? null });
}

export async function renameWorkflowRun(workflowRunId: number, name: string | null) {
  return command<TaskResponse>("rename_workflow_run", { workflowRunId, name });
}

export type ArtifactUploadRequest = {
  run_id: number;
  workflow_node_run_id?: number | null;
};

export type ArtifactDownloadResult = { saved_to: string | null };

export async function fetchAllArtifacts() {
  return command<RunArtifact[]>("fetch_all_artifacts");
}

export async function uploadArtifactFromPath(request: ArtifactUploadRequest) {
  return command<RunArtifact>("upload_artifact", { request });
}

export async function downloadArtifactToPath(artifactId: number, defaultName: string) {
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

export async function downloadArtifactInBrowser(artifactId: number, defaultName: string) {
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

export async function markNotificationRead(notificationId: number) {
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

export async function fetchCredentials() {
  return command<CredentialSummary[]>("fetch_credentials");
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

export async function approveApproval(approvalId: number) {
  return command<any>("approve_approval", { approvalId });
}

export async function rejectApproval(approvalId: number) {
  return command<any>("reject_approval", { approvalId });
}
