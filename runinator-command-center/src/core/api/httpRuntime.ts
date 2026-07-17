// Web-mode runtime adapter. Translates Tauri command names + args into HTTP
// requests against runinator-ws. Used when the SPA runs outside of Tauri
// (browser, in-cluster deployment).
//
// Path conventions: all paths are relative to apiBaseUrl(). In production the
// SPA is served by an nginx pod that reverse-proxies "/api/*" to runinator-ws,
// so apiBaseUrl() returns "/api". In `vite dev` we either rely on the dev
// server proxy (default) or honor VITE_RUNINATOR_WS_URL for direct override.

import { displayValue } from "../utils/values";

type Method = "GET" | "POST" | "PATCH" | "DELETE";

type CommandArgs = Record<string, unknown> | undefined;

interface HttpDescriptor {
  method: Method | ((args: CommandArgs) => Method);
  path: (args: CommandArgs) => string;
  body?: (args: CommandArgs) => unknown;
  headers?: (args: CommandArgs) => Record<string, string>;
  transform?: (raw: unknown) => unknown;
  accept404?: boolean;
}

const WORKFLOW_JSON_IMPORT_RISK_HEADER = "x-runinator-json-workflow-risk";
const WORKFLOW_JSON_IMPORT_RISK_ACK = "system-breakage-possible";

// access token presented as `Authorization: Bearer …` in web mode; also appended to WS urls.
let authToken: string | null = null;

export function setHttpAuthToken(token: string | null): void {
  authToken = token && token.length > 0 ? token : null;
}

export function httpAuthToken(): string | null {
  return authToken;
}

function authHeaders(): Record<string, string> {
  return authToken ? { authorization: `Bearer ${authToken}` } : {};
}

function arg(args: CommandArgs, key: string): unknown {
  if (!args || !(key in args)) {
    throw new Error(`Missing argument '${key}'`);
  }

  return args[key];
}

function argOpt(args: CommandArgs, key: string): unknown {
  if (!args) {
    return undefined;
  }

  return args[key];
}

function escape(part: unknown): string {
  return encodeURIComponent(String(part));
}

const REGISTRY: Record<string, HttpDescriptor> = {
  auth_config: { method: "GET", path: () => "auth/config" },
  auth_me: { method: "GET", path: () => "auth/me" },
  login: {
    method: "POST",
    path: () => "auth/login",
    body: (args) => ({ username: arg(args, "username"), password: arg(args, "password") }),
  },
  refresh_session: {
    method: "POST",
    path: () => "auth/refresh",
    body: (args) => ({ refresh_token: arg(args, "refreshToken") }),
  },
  logout: {
    method: "POST",
    path: () => "auth/logout",
    body: (args) => ({ refresh_token: arg(args, "refreshToken") }),
  },
  list_workflow_grants: {
    method: "GET",
    path: (args) => `workflows/${escape(arg(args, "workflowId"))}/grants`,
  },
  create_workflow_grant: {
    method: "POST",
    path: (args) => `workflows/${escape(arg(args, "workflowId"))}/grants`,
    body: (args) => ({
      principal_type: arg(args, "principalType"),
      principal_id: arg(args, "principalId"),
      permission: arg(args, "permission"),
    }),
  },
  revoke_workflow_grant: {
    method: "DELETE",
    path: (args) =>
      `workflows/${escape(arg(args, "workflowId"))}/grants/${escape(arg(args, "grantId"))}`,
  },
  list_dead_letters: {
    method: "GET",
    path: (args) => {
      const params = new URLSearchParams();
      const channel = argOpt(args, "channel");

      if (typeof channel === "string" && channel) {
        params.set("channel", channel);
      }

      const limit = argOpt(args, "limit");

      if (limit != null) {
        params.set("limit", displayValue(limit));
      }

      const query = params.toString();
      return query ? `dead_letters?${query}` : "dead_letters";
    },
  },
  list_audit_log: {
    method: "GET",
    path: (args) => {
      const params = new URLSearchParams();
      const actorId = argOpt(args, "actorId");

      if (typeof actorId === "string" && actorId) {
        params.set("actor_id", actorId);
      }

      const action = argOpt(args, "action");

      if (typeof action === "string" && action) {
        params.set("action", action);
      }

      const limit = argOpt(args, "limit");

      if (limit != null) {
        params.set("limit", displayValue(limit));
      }

      const query = params.toString();
      return query ? `audit_log?${query}` : "audit_log";
    },
  },
  list_users: { method: "GET", path: () => "users" },
  create_user: {
    method: "POST",
    path: () => "users",
    body: (args) => arg(args, "request"),
  },
  update_user: {
    method: "PATCH",
    path: (args) => `users/${escape(arg(args, "userId"))}`,
    body: (args) => arg(args, "request"),
  },
  delete_user: {
    method: "DELETE",
    path: (args) => `users/${escape(arg(args, "userId"))}`,
  },
  list_teams: { method: "GET", path: () => "teams" },
  create_team: {
    method: "POST",
    path: () => "teams",
    body: (args) => ({ name: arg(args, "name") }),
  },
  update_team: {
    method: "PATCH",
    path: (args) => `teams/${escape(arg(args, "teamId"))}`,
    body: (args) => ({ name: arg(args, "name") }),
  },
  delete_team: {
    method: "DELETE",
    path: (args) => `teams/${escape(arg(args, "teamId"))}`,
  },
  list_team_members: {
    method: "GET",
    path: (args) => `teams/${escape(arg(args, "teamId"))}/members`,
  },
  list_user_teams: {
    method: "GET",
    path: (args) => `users/${escape(arg(args, "userId"))}/teams`,
  },
  add_team_member: {
    method: "POST",
    path: (args) => `teams/${escape(arg(args, "teamId"))}/members`,
    body: (args) => ({ user_id: arg(args, "userId") }),
  },
  remove_team_member: {
    method: "DELETE",
    path: (args) =>
      `teams/${escape(arg(args, "teamId"))}/members/${escape(arg(args, "userId"))}`,
  },
  list_api_keys: { method: "GET", path: () => "api_keys" },
  create_api_key: {
    method: "POST",
    path: () => "api_keys",
    body: (args) => arg(args, "request"),
  },
  update_api_key: {
    method: "PATCH",
    path: (args) => `api_keys/${escape(arg(args, "keyId"))}`,
    body: (args) => arg(args, "request"),
  },
  revoke_api_key: {
    method: "DELETE",
    path: (args) => `api_keys/${escape(arg(args, "keyId"))}`,
  },
  rotate_api_key: {
    method: "POST",
    path: (args) => `api_keys/${escape(arg(args, "keyId"))}/rotate`,
  },
  fetch_workflows: { method: "GET", path: () => "workflows" },
  save_workflow: {
    method: (args) => {
      const workflow = arg(args, "workflow") as { id?: string | null };
      return workflow.id != null ? "PATCH" : "POST";
    },
    path: (args) => {
      const workflow = arg(args, "workflow") as { id?: string | null };
      return workflow.id != null ? `workflows/${escape(workflow.id)}` : "workflows";
    },
    body: (args) => arg(args, "workflow"),
  },
  simulate_workflow: {
    method: "POST",
    path: () => "workflows/simulate",
    body: (args) => arg(args, "request"),
  },
  save_workflow_bundle: {
    method: "POST",
    path: () => "workflows/import",
    body: (args) => arg(args, "request"),
    headers: () => ({ [WORKFLOW_JSON_IMPORT_RISK_HEADER]: WORKFLOW_JSON_IMPORT_RISK_ACK }),
  },
  save_workflow_wdl: {
    method: "POST",
    path: () => "wdl/import",
    body: (args) => arg(args, "request"),
  },
  delete_workflow: {
    method: "DELETE",
    path: (args) => `workflows/${escape(arg(args, "workflowId"))}`,
  },
  duplicate_workflow: {
    method: "POST",
    path: (args) => {
      const bump = argOpt(args, "bump") ?? "minor";
      return `workflows/${escape(arg(args, "workflowId"))}/duplicate?bump=${escape(bump)}`;
    },
  },
  fetch_workflow_triggers: {
    method: "GET",
    path: (args) => `workflows/${escape(arg(args, "workflowId"))}/triggers`,
  },
  save_workflow_trigger: {
    method: (args) => (arg(args, "creating") === true ? "POST" : "PATCH"),
    path: (args) => {
      const creating = arg(args, "creating") === true;
      const trigger = arg(args, "trigger") as { id?: string | null; workflow_id: string };

      if (creating) {
        return `workflows/${escape(trigger.workflow_id)}/triggers`;
      }

      if (trigger.id == null) {
        throw new Error("missing workflow trigger id");
      }

      return `workflow_triggers/${escape(trigger.id)}`;
    },
    body: (args) => arg(args, "trigger"),
  },
  delete_workflow_trigger: {
    method: "DELETE",
    path: (args) => `workflow_triggers/${escape(arg(args, "triggerId"))}`,
  },
  fetch_pipelines: { method: "GET", path: () => "pipelines" },
  fetch_pipeline: {
    method: "GET",
    path: (args) => `pipelines/${escape(arg(args, "pipelineId"))}`,
  },
  save_pipeline: {
    method: (args) => {
      const pipeline = arg(args, "pipeline") as { id?: string | null };
      return pipeline.id != null ? "PATCH" : "POST";
    },
    path: (args) => {
      const pipeline = arg(args, "pipeline") as { id?: string | null };
      return pipeline.id != null ? `pipelines/${escape(pipeline.id)}` : "pipelines";
    },
    body: (args) => arg(args, "pipeline"),
  },
  delete_pipeline: {
    method: "DELETE",
    path: (args) => `pipelines/${escape(arg(args, "pipelineId"))}`,
  },
  set_pipeline_owner: {
    method: "PATCH",
    path: (args) => `pipelines/${escape(arg(args, "pipelineId"))}/owner`,
    body: (args) => ({ org_id: argOpt(args, "orgId") ?? null }),
  },
  fetch_run_chunks: {
    method: "GET",
    path: (args) => `runs/${escape(arg(args, "runId"))}/chunks?limit=500`,
  },
  fetch_run_artifacts: {
    method: "GET",
    path: (args) => `runs/${escape(arg(args, "runId"))}/artifacts`,
  },
  fetch_workflow_node_run_chunks: {
    method: "GET",
    path: (args) => `workflow_node_runs/${escape(arg(args, "nodeRunId"))}/chunks?limit=500`,
  },
  fetch_workflow_node_run_artifacts: {
    method: "GET",
    path: (args) => `workflow_node_runs/${escape(arg(args, "nodeRunId"))}/artifacts`,
  },
  fetch_workflow_run_artifacts: {
    method: "GET",
    path: (args) => `workflow_runs/${escape(arg(args, "workflowRunId"))}/artifacts`,
  },
  create_workflow_run: {
    method: "POST",
    path: (args) => `workflows/${escape(arg(args, "workflowId"))}/runs`,
    body: (args) => ({
      debug: argOpt(args, "debug") ?? false,
      parameters: argOpt(args, "parameters") ?? {},
    }),
    transform: extractWorkflowRunId,
  },
  step_workflow_run: workflowRunDebugAction("step"),
  continue_workflow_run: workflowRunDebugAction("continue"),
  cancel_workflow_run: workflowRunAction("cancel"),
  pause_workflow_run: workflowRunAction("pause"),
  resume_workflow_run: workflowRunAction("resume"),
  patch_workflow_run_debug: {
    method: "PATCH",
    path: (args) => `workflow_runs/${escape(arg(args, "workflowRunId"))}/debug`,
    body: (args) => arg(args, "patch"),
  },
  run_to_cursor_workflow_run: {
    method: "POST",
    path: (args) =>
      `workflow_runs/${escape(arg(args, "workflowRunId"))}/debug/run_to_cursor`,
    body: (args) => ({ node_id: arg(args, "nodeId") }),
  },
  skip_workflow_node: {
    method: "POST",
    path: (args) => `workflow_runs/${escape(arg(args, "workflowRunId"))}/debug/skip`,
    body: (args) => ({
      output_json: arg(args, "outputJson"),
      message: argOpt(args, "message") ?? null,
    }),
  },
  resolve_workflow_input: {
    method: "POST",
    path: (args) => `workflow_node_runs/${escape(arg(args, "nodeRunId"))}/input`,
    body: (args) => ({
      output_json: arg(args, "outputJson"),
      resolved_by: argOpt(args, "resolvedBy") ?? null,
      message: argOpt(args, "message") ?? null,
    }),
  },
  rerun_workflow_node: {
    method: "POST",
    path: (args) => `workflow_runs/${escape(arg(args, "workflowRunId"))}/debug/rerun_node`,
    body: (args) => ({ parameters: arg(args, "parameters") }),
  },
  fetch_supervisor_status: {
    method: "GET",
    path: () => "supervisor/status",
    accept404: true,
  },
  replay_workflow_run: {
    method: "POST",
    path: (args) => `workflow_runs/${escape(arg(args, "workflowRunId"))}/replay`,
    body: (args) => ({ from_step_id: argOpt(args, "fromStepId") ?? null }),
    transform: extractWorkflowRunId,
  },
  rename_workflow_run: {
    method: "POST",
    path: (args) => `workflow_runs/${escape(arg(args, "workflowRunId"))}/rename`,
    body: (args) => ({ name: argOpt(args, "name") ?? null }),
  },
  fetch_workflow_runs: {
    method: "GET",
    path: (args) => {
      const workflowId = argOpt(args, "workflowId");
      return workflowId != null
        ? `workflow_runs?workflow_id=${escape(workflowId)}`
        : "workflow_runs";
    },
  },
  fetch_workflow_run: {
    method: "GET",
    path: (args) => `workflow_runs/${escape(arg(args, "workflowRunId"))}`,
    transform: (raw) => {
      const body = raw as { run?: unknown; nodes?: unknown };

      if (body.run == null) {
        throw new Error("missing workflow run");
      }

      return { run: body.run, nodes: body.nodes ?? [] };
    },
  },
  fetch_resource_records: {
    method: "GET",
    path: (args) => String(arg(args, "endpoint")),
  },
  complete_wdl: {
    method: "POST",
    path: () => "wdl/complete",
    body: (args) => arg(args, "request"),
  },
  hover_wdl: {
    method: "POST",
    path: () => "wdl/hover",
    body: (args) => arg(args, "request"),
  },
  compile_wdl: {
    method: "POST",
    path: () => "wdl/compile",
    body: (args) => ({ source: arg(args, "source"), enabled: arg(args, "enabled") }),
  },
  analyze_wdl: {
    method: "POST",
    path: () => "wdl/analyze",
    body: (args) => ({
      source: arg(args, "source"),
      source_path: argOpt(args, "sourcePath") ?? null,
    }),
  },
  format_wdl: {
    method: "POST",
    path: () => "wdl/format",
    body: (args) => ({ source: arg(args, "source") }),
  },
  decompile_to_wdl: {
    method: "POST",
    path: () => "wdl/decompile",
    body: (args) => ({ workflow: arg(args, "workflow") }),
  },
  evaluate_expression: {
    method: "POST",
    path: () => "wdl/evaluate",
    body: (args) => ({ expression: arg(args, "expression"), context: arg(args, "context") }),
  },
  fetch_providers: { method: "GET", path: () => "providers" },
  fetch_node_kinds: { method: "GET", path: () => "node-kinds" },
  fetch_trigger_kinds: { method: "GET", path: () => "trigger-kinds" },
  fetch_enum_catalogs: { method: "GET", path: () => "catalog/enums" },
  fetch_replicas: { method: "GET", path: () => "replicas" },
  fetch_node_backends: { method: "GET", path: () => "nodes/backends" },
  fetch_nodes: { method: "GET", path: () => "nodes" },
  scale_nodes: { method: "POST", path: () => "nodes/scale", body: (args) => arg(args, "request") },
  stop_node: { method: "POST", path: () => "nodes/stop", body: (args) => arg(args, "request") },
  fetch_credentials: { method: "GET", path: () => "credentials" },
  fetch_credential: {
    method: "GET",
    path: (args) =>
      `credentials?scope=${escape(arg(args, "scope"))}&name=${escape(arg(args, "name"))}&kind=${escape(arg(args, "kind"))}`,
  },
  save_credential: {
    method: "POST",
    path: () => "credentials",
    body: (args) => arg(args, "request"),
  },
  delete_credential: {
    method: "DELETE",
    path: (args) =>
      `credentials?scope=${escape(arg(args, "scope"))}&name=${escape(arg(args, "name"))}&kind=${escape(arg(args, "kind"))}`,
  },
  approve_approval: {
    method: "POST",
    path: (args) => `approvals/${escape(arg(args, "approvalId"))}/approve`,
    body: () => ({}),
  },
  reject_approval: {
    method: "POST",
    path: (args) => `approvals/${escape(arg(args, "approvalId"))}/reject`,
    body: () => ({}),
  },
  fetch_all_artifacts: { method: "GET", path: () => "artifacts" },
  fetch_notifications: {
    method: "GET",
    path: (args) => {
      const limit = (argOpt(args, "limit") as number | undefined) ?? 200;
      const unreadOnly = argOpt(args, "unreadOnly");
      const unread = unreadOnly === true;
      const base = `notifications?limit=${escape(limit)}`;
      return unread ? `${base}&unread=true` : base;
    },
  },
  mark_notification_read: {
    method: "POST",
    path: (args) => `notifications/${escape(arg(args, "notificationId"))}/mark_read`,
    body: () => ({}),
  },
  mark_all_notifications_read: {
    method: "POST",
    path: () => "notifications/mark_all_read",
    body: () => ({}),
  },
  delete_notification: {
    method: "DELETE",
    path: (args) => `notifications/${escape(arg(args, "notificationId"))}`,
  },
  delete_artifact: {
    method: "DELETE",
    path: (args) => `artifacts/${escape(arg(args, "artifactId"))}`,
  },
  delete_gate: {
    method: "DELETE",
    path: (args) => `gates/${escape(arg(args, "gateId"))}`,
  },
  delete_automation_event: {
    method: "DELETE",
    path: (args) => `automation_events/${escape(arg(args, "eventId"))}`,
  },
  fetch_replica_samples: {
    method: "GET",
    path: (args) => {
      const base = `replicas/${escape(arg(args, "replicaId"))}/samples`;
      const since = argOpt(args, "sinceSeconds");
      return since ? `${base}?since_seconds=${escape(since)}` : base;
    },
  },
  set_workflow_owner: {
    method: "PATCH",
    path: (args) => `workflows/${escape(arg(args, "workflowId"))}/owner`,
    body: (args) => ({ org_id: argOpt(args, "orgId") ?? null }),
  },
  // --- organizations (tenants), membership, resource allocation, and billing ---
  list_my_orgs: { method: "GET", path: () => "orgs/me" },
  list_orgs: { method: "GET", path: () => "orgs" },
  create_org: {
    method: "POST",
    path: () => "orgs",
    body: (args) => ({ name: arg(args, "name") }),
  },
  switch_org: {
    method: "POST",
    path: () => "auth/switch-org",
    body: (args) => ({ org_id: arg(args, "orgId") }),
  },
  list_org_members: {
    method: "GET",
    path: (args) => `orgs/${escape(arg(args, "orgId"))}/members`,
    headers: (args) => ({ "x-org-id": String(arg(args, "orgId")) }),
  },
  add_org_member: {
    method: "POST",
    path: (args) => `orgs/${escape(arg(args, "orgId"))}/members`,
    headers: (args) => ({ "x-org-id": String(arg(args, "orgId")) }),
    body: (args) => ({ user_id: arg(args, "userId"), role: arg(args, "role") }),
  },
  update_org_member: {
    method: "PATCH",
    path: (args) =>
      `orgs/${escape(arg(args, "orgId"))}/members/${escape(arg(args, "userId"))}`,
    headers: (args) => ({ "x-org-id": String(arg(args, "orgId")) }),
    body: (args) => ({ role: arg(args, "role") }),
  },
  remove_org_member: {
    method: "DELETE",
    path: (args) =>
      `orgs/${escape(arg(args, "orgId"))}/members/${escape(arg(args, "userId"))}`,
    headers: (args) => ({ "x-org-id": String(arg(args, "orgId")) }),
  },
  fetch_rate_card: { method: "GET", path: () => "rate-card" },
  fetch_org_nodes: {
    method: "GET",
    path: (args) => `orgs/${escape(arg(args, "orgId"))}/nodes`,
    headers: (args) => ({ "x-org-id": String(arg(args, "orgId")) }),
  },
  scale_org_nodes: {
    method: "POST",
    path: (args) => `orgs/${escape(arg(args, "orgId"))}/nodes/scale`,
    headers: (args) => ({ "x-org-id": String(arg(args, "orgId")) }),
    body: (args) => arg(args, "request"),
  },
  fetch_org_quota: {
    method: "GET",
    path: (args) => `orgs/${escape(arg(args, "orgId"))}/quota`,
    headers: (args) => ({ "x-org-id": String(arg(args, "orgId")) }),
  },
  fetch_org_usage: {
    method: "GET",
    path: (args) => `orgs/${escape(arg(args, "orgId"))}/usage`,
    headers: (args) => ({ "x-org-id": String(arg(args, "orgId")) }),
  },
};

function workflowRunAction(action: string): HttpDescriptor {
  return {
    method: "POST",
    path: (args) => `workflow_runs/${escape(arg(args, "workflowRunId"))}/${action}`,
    body: () => ({}),
  };
}

function workflowRunDebugAction(action: string): HttpDescriptor {
  return {
    method: "POST",
    path: (args) => `workflow_runs/${escape(arg(args, "workflowRunId"))}/debug/${action}`,
    body: () => ({}),
  };
}

function extractWorkflowRunId(raw: unknown): { id: string } {
  const body = raw as { run?: { id?: string } } | null;
  const id = body?.run?.id;

  if (typeof id !== "string" || id.length === 0) {
    throw new Error("missing workflow run id");
  }

  return { id };
}

export function apiBaseUrl(): string {
  const override = (import.meta as { env?: Record<string, string | undefined> }).env
    ?.VITE_RUNINATOR_WS_URL;

  if (override && override.trim().length > 0) {
    return override.replace(/\/+$/, "");
  }

  return "/api";
}

export function wsBaseUrl(): string {
  const override = (import.meta as { env?: Record<string, string | undefined> }).env
    ?.VITE_RUNINATOR_WS_URL;

  if (override && override.trim().length > 0) {
    return override.replace(/\/+$/, "");
  }

  if (typeof window === "undefined") {
    return "";
  }

  return window.location.origin;
}

// unauthenticated reachability probe against the public /health endpoint; resolves false when the
// backend/proxy cannot be reached or reports unhealthy. used to detect idle outages/recovery.
export async function pingBackendHealth(): Promise<boolean> {
  try {
    const response = await fetch(`${apiBaseUrl()}/health`, { method: "GET", cache: "no-store" });
    return response.ok;
  } catch {
    return false;
  }
}

export async function invokeViaHttp<T>(name: string, args?: Record<string, unknown>): Promise<T> {
  if (name === "get_service_status") {
    return { service_url: wsBaseUrl() || null } as unknown as T;
  }

  if (name === "start_service_discovery") {
    return undefined as unknown as T;
  }

  if (name === "set_access_token") {
    setHttpAuthToken((args?.token as string | undefined) ?? null);
    return undefined as unknown as T;
  }

  if (name === "upload_artifact" || name === "download_artifact") {
    throw new Error(
      `${name} is not available in web mode; use uploadArtifactBlob/downloadArtifactBlob instead`,
    );
  }

  if (!(name in REGISTRY)) {
    throw new Error(`Unknown command in web mode: ${name}`);
  }

  const descriptor = REGISTRY[name];

  const base = apiBaseUrl();
  const path = descriptor.path(args).replace(/^\/+/, "");
  const url = `${base}/${path}`;
  const method =
    typeof descriptor.method === "function" ? descriptor.method(args) : descriptor.method;
  const init: RequestInit = { method };
  const headers: Record<string, string> = {
    ...authHeaders(),
    ...(descriptor.headers ? descriptor.headers(args) : {}),
  };

  if (descriptor.body) {
    headers["content-type"] = "application/json";
    init.body = JSON.stringify(descriptor.body(args));
  }

  if (Object.keys(headers).length > 0) {
    init.headers = headers;
  }

  const response = await fetch(url, init);

  if (response.status === 404 && descriptor.accept404) {
    return (await response.json()) as T;
  }

  if (!response.ok) {
    const text = await response.text().catch(() => "");
    throw new Error(`${method} ${url} -> ${String(response.status)}: ${text}`);
  }

  if (response.status === 204) {
    return undefined as unknown as T;
  }

  const raw: unknown = await response.json();

  // workflow imports: after import, re-export the first saved workflow to
  // hydrate the bundle with server-assigned ids — mirrors the Tauri command.
  if (name === "save_workflow_bundle" || name === "save_workflow_wdl") {
    const saved = raw as { workflows?: { id?: string | null }[] };
    const id = saved.workflows?.[0]?.id;

    if (id == null) {
      return saved as unknown as T;
    }

    const exportResp = await fetch(`${base}/workflows/${escape(id)}/export`, {
      headers: authHeaders(),
    });

    if (!exportResp.ok) {
      const text = await exportResp.text().catch(() => "");
      throw new Error(`GET workflows/${id}/export -> ${String(exportResp.status)}: ${text}`);
    }

    return (await exportResp.json()) as T;
  }

  return (descriptor.transform ? descriptor.transform(raw) : raw) as T;
}
