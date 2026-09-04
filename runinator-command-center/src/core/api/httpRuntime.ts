// Web-mode runtime adapter. Translates Tauri command names + args into HTTP
// requests against runinator-ws. Used when the SPA runs outside of Tauri
// (browser, in-cluster deployment).
//
// Path conventions: all paths are relative to apiBaseUrl(). In production the
// SPA is served by an nginx pod that reverse-proxies "/api/*" to runinator-ws,
// so apiBaseUrl() returns "/api". In `vite dev` we either rely on the dev
// server proxy (default) or honor VITE_RUNINATOR_WS_URL for direct override.

import { displayValue } from "../utils/values";
import { createZip } from "../utils/zip";

type Method = "GET" | "POST" | "PUT" | "PATCH" | "DELETE";

type CommandArgs = Record<string, unknown> | undefined;

interface HttpDescriptor {
  method: Method | ((args: CommandArgs) => Method);
  path: (args: CommandArgs) => string;
  body?: (args: CommandArgs) => unknown;
  /// a body sent verbatim under its own content type, for the endpoints that take bytes rather
  /// than json. mutually exclusive with `body`.
  rawBody?: (args: CommandArgs) => { body: BodyInit; contentType: string };
  headers?: (args: CommandArgs) => Record<string, string>;
  transform?: (raw: unknown) => unknown;
  accept404?: boolean;
}

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

// Store who resolved an approval and what they said. Every field is optional, so a UI that only clicks
// "approve" sends the same body it always did.
const REGISTRY: Record<string, HttpDescriptor> = {
  auth_config: { method: "GET", path: () => "auth/config" },
  auth_me: { method: "GET", path: () => "auth/me" },
  update_current_user: {
    method: "PATCH",
    path: () => "auth/me",
    body: (args) => arg(args, "request"),
  },
  change_current_password: {
    method: "POST",
    path: () => "auth/me/password",
    body: (args) => arg(args, "request"),
  },
  list_current_sessions: { method: "GET", path: () => "auth/sessions" },
  revoke_current_session: {
    method: "DELETE",
    path: (args) => `auth/sessions/${escape(arg(args, "sessionId"))}`,
  },
  revoke_other_sessions: { method: "POST", path: () => "auth/sessions/revoke-others" },
  list_personal_api_keys: { method: "GET", path: () => "auth/me/api-keys" },
  list_personal_api_key_scopes: { method: "GET", path: () => "auth/me/api-key-scopes" },
  create_personal_api_key: {
    method: "POST",
    path: () => "auth/me/api-keys",
    body: (args) => arg(args, "request"),
  },
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
  fetch_auth_settings: { method: "GET", path: () => "auth/settings" },
  save_auth_settings: {
    method: "PUT",
    path: () => "auth/settings",
    body: (args) => ({ max_refreshes: arg(args, "maxRefreshes") }),
  },
  fetch_server_settings: { method: "GET", path: () => "server/settings" },
  save_server_settings: {
    method: "PUT",
    path: () => "server/settings",
    body: (args) => arg(args, "settings"),
  },
  list_resource_grants: {
    method: "GET",
    path: (args) =>
      `authz/resources/${escape(arg(args, "resourceType"))}/${escape(arg(args, "resourceId"))}/grants`,
  },
  fetch_resource_owner: {
    method: "GET",
    path: (args) =>
      `authz/resources/${escape(arg(args, "resourceType"))}/${escape(arg(args, "resourceId"))}/owner`,
  },
  create_resource_grant: {
    method: "POST",
    path: (args) =>
      `authz/resources/${escape(arg(args, "resourceType"))}/${escape(arg(args, "resourceId"))}/grants`,
    body: (args) => ({
      principal_type: arg(args, "principalType"),
      principal_id: arg(args, "principalId"),
      permission: arg(args, "permission"),
    }),
  },
  revoke_resource_grant: {
    method: "DELETE",
    path: (args) =>
      `authz/resources/${escape(arg(args, "resourceType"))}/${escape(arg(args, "resourceId"))}/grants/${escape(arg(args, "grantId"))}`,
  },
  transfer_resource_owner: {
    method: "POST",
    path: (args) =>
      `authz/resources/${escape(arg(args, "resourceType"))}/${escape(arg(args, "resourceId"))}/owner`,
    body: (args) => ({
      owner: {
        kind: arg(args, "scopeKind"),
        id: argOpt(args, "scopeId") ?? null,
      },
    }),
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
  list_broker_messages: {
    method: "GET",
    path: (args) => {
      const params = new URLSearchParams();
      const workflowRunId = argOpt(args, "workflowRunId");
      const pipelineRunId = argOpt(args, "pipelineRunId");
      const channel = argOpt(args, "channel");
      const limit = argOpt(args, "limit");

      if (typeof workflowRunId === "string" && workflowRunId) {
        params.set("workflow_run_id", workflowRunId);
      }

      if (typeof pipelineRunId === "string" && pipelineRunId) {
        params.set("pipeline_run_id", pipelineRunId);
      }

      if (typeof channel === "string" && channel) {
        params.set("channel", channel);
      }

      if (limit != null) {
        params.set("limit", displayValue(limit));
      }

      const query = params.toString();
      return query ? `broker_messages?${query}` : "broker_messages";
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
    body: (args) => ({ user_id: arg(args, "userId"), role: arg(args, "role") }),
  },
  remove_team_member: {
    method: "DELETE",
    path: (args) => `teams/${escape(arg(args, "teamId"))}/members/${escape(arg(args, "userId"))}`,
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
    path: () => "packs/import?overwrite=true",
    rawBody: (args) => ({
      body: createZip([{ name: "workflows.json", content: JSON.stringify(arg(args, "request")) }]),
      contentType: "application/zip",
    }),
  },
  save_workflow_rexrap: {
    method: "POST",
    path: () => "rexrap/import",
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
  fetch_workflow_revisions: {
    method: "GET",
    path: (args) => {
      const limit = argOpt(args, "limit");
      const base = `workflows/${escape(arg(args, "workflowId"))}/revisions`;
      return typeof limit === "number" ? `${base}?limit=${escape(limit)}` : base;
    },
  },
  fetch_workflow_revision: {
    method: "GET",
    path: (args) =>
      `workflows/${escape(arg(args, "workflowId"))}/revisions/${escape(String(arg(args, "revision")))}`,
  },
  restore_workflow_revision: {
    method: "POST",
    path: (args) =>
      `workflows/${escape(arg(args, "workflowId"))}/revisions/${escape(String(arg(args, "revision")))}/restore`,
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
  fetch_due_triggers: { method: "GET", path: () => "workflow_triggers/due" },
  create_trigger_run: {
    method: "POST",
    path: (args) => `workflow_triggers/${escape(arg(args, "triggerId"))}/runs`,
    body: (args) => ({
      parameters: argOpt(args, "parameters") ?? {},
      debug: argOpt(args, "debug") === true,
    }),
  },
  export_workflow_bundle: {
    method: "GET",
    path: (args) => {
      const workflowId = argOpt(args, "workflowId");
      return workflowId == null ? "workflows/export" : `workflows/${escape(workflowId)}/export`;
    },
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
  create_pipeline_run: {
    method: "POST",
    path: (args) => `pipelines/${escape(arg(args, "pipelineId"))}/runs`,
    body: (args) => ({ parameters: argOpt(args, "parameters") ?? {} }),
  },
  fetch_pipeline_runs: { method: "GET", path: () => "pipeline_runs" },
  fetch_orchestrations: {
    method: "GET",
    path: (args) => {
      const filters = (argOpt(args, "filters") ?? {}) as Record<string, unknown>;
      const params = new URLSearchParams();

      for (const [key, value] of Object.entries(filters)) {
        if (typeof value === "string" && value !== "") {
          params.set(key, value);
        } else if (typeof value === "number" || typeof value === "boolean") {
          params.set(key, String(value));
        }
      }

      const query = params.toString();

      return query ? `orchestrations?${query}` : "orchestrations";
    },
  },
  fetch_orchestration: {
    method: "GET",
    path: (args) => `orchestrations/${escape(arg(args, "orchestrationId"))}`,
  },
  fetch_orchestration_epochs: {
    method: "GET",
    path: (args) => `orchestrations/${escape(arg(args, "orchestrationId"))}/epochs`,
  },
  fetch_orchestration_events: {
    method: "GET",
    path: (args) => `orchestrations/${escape(arg(args, "orchestrationId"))}/events`,
  },
  fetch_orchestration_evidence: {
    method: "GET",
    path: (args) => `orchestrations/${escape(arg(args, "orchestrationId"))}/evidence`,
  },
  fetch_orchestration_commands: {
    method: "GET",
    path: (args) => `orchestrations/${escape(arg(args, "orchestrationId"))}/commands`,
  },
  fetch_orchestration_workspaces: {
    method: "GET",
    path: (args) => `orchestrations/${escape(arg(args, "orchestrationId"))}/workspaces`,
  },
  fetch_orchestration_aliases: {
    method: "GET",
    path: (args) => `orchestrations/${escape(arg(args, "orchestrationId"))}/aliases`,
  },
  add_orchestration_alias: {
    method: "POST",
    path: (args) => `orchestrations/${escape(arg(args, "orchestrationId"))}/aliases`,
    body: (args) => ({
      source: arg(args, "source"),
      scope: arg(args, "scope"),
      correlation_key: arg(args, "correlationKey"),
    }),
  },
  delete_orchestration_alias: {
    method: "DELETE",
    path: (args) =>
      `orchestrations/${escape(arg(args, "orchestrationId"))}/aliases/${escape(arg(args, "aliasId"))}`,
  },
  fetch_external_operations: {
    method: "GET",
    path: (args) => `orchestrations/${escape(arg(args, "orchestrationId"))}/operations`,
  },
  resolve_external_operation: {
    method: "POST",
    path: (args) =>
      `orchestrations/${escape(arg(args, "orchestrationId"))}/operations/${escape(arg(args, "operationId"))}/resolve`,
    body: (args) => ({
      resolution: arg(args, "resolution"),
      reason: arg(args, "reason"),
      receipt: argOpt(args, "receipt") ?? null,
    }),
  },
  fetch_adapter_kinds: { method: "GET", path: () => "orchestrations/adapters/kinds" },
  fetch_adapters: { method: "GET", path: () => "orchestrations/adapters" },
  fetch_adapter: {
    method: "GET",
    path: (args) => `orchestrations/adapters/${escape(arg(args, "adapterId"))}`,
  },
  fetch_adapter_revisions: {
    method: "GET",
    path: (args) => `orchestrations/adapters/${escape(arg(args, "adapterId"))}/revisions`,
  },
  fetch_adapter_poll_status: {
    method: "GET",
    path: (args) => `orchestrations/adapters/${escape(arg(args, "adapterId"))}/poll-status`,
  },
  apply_adapter: {
    method: (args) => (argOpt(args, "adapterId") ? "POST" : "POST"),
    path: (args) => {
      const adapterId = argOpt(args, "adapterId");
      return adapterId ? `orchestrations/adapters/${escape(adapterId)}` : "orchestrations/adapters";
    },
    body: (args) => arg(args, "adapter"),
  },
  set_adapter_enabled: {
    method: "POST",
    path: (args) => `orchestrations/adapters/${escape(arg(args, "adapterId"))}/enabled`,
    body: (args) => ({ enabled: arg(args, "enabled") }),
  },
  delete_adapter: {
    method: "DELETE",
    path: (args) => `orchestrations/adapters/${escape(arg(args, "adapterId"))}`,
  },
  test_adapter: {
    method: "POST",
    path: (args) => `orchestrations/adapters/${escape(arg(args, "adapterId"))}/test`,
    body: (args) => ({
      headers: argOpt(args, "headers") ?? {},
      body_base64: arg(args, "bodyBase64"),
    }),
  },
  fetch_adapter_health: { method: "GET", path: () => "orchestrations/adapters/health" },
  reload_adapter_host: {
    method: "POST",
    path: () => "orchestrations/adapters/reload",
    body: () => ({}),
  },
  send_orchestration_intent: {
    method: "POST",
    path: (args) => `orchestrations/${escape(arg(args, "orchestrationId"))}/intents`,
    body: (args) => ({
      intent: arg(args, "intent"),
      reason: arg(args, "reason"),
      payload: argOpt(args, "payload") ?? {},
      idempotency_key: arg(args, "idempotencyKey"),
    }),
  },
  requeue_orchestration: {
    method: "POST",
    path: (args) => `orchestrations/${escape(arg(args, "orchestrationId"))}/requeue`,
    body: (args) => ({
      reason: arg(args, "reason"),
      idempotency_key: arg(args, "idempotencyKey"),
    }),
  },
  fetch_pipeline_run: {
    method: "GET",
    path: (args) => `pipeline_runs/${escape(arg(args, "pipelineRunId"))}`,
  },
  delete_pipeline_run: {
    method: "DELETE",
    path: (args) => `pipeline_runs/${escape(arg(args, "pipelineRunId"))}`,
  },
  cancel_pipeline_run: {
    method: "POST",
    path: (args) => `pipeline_runs/${escape(arg(args, "pipelineRunId"))}/cancel`,
    body: (args) => ({
      reason: argOpt(args, "overrideReason") ?? null,
      idempotency_key: argOpt(args, "idempotencyKey") ?? null,
    }),
  },
  pause_pipeline_run: {
    method: "POST",
    path: (args) => `pipeline_runs/${escape(arg(args, "pipelineRunId"))}/pause`,
    body: (args) => ({
      reason: argOpt(args, "overrideReason") ?? null,
      idempotency_key: argOpt(args, "idempotencyKey") ?? null,
    }),
  },
  resume_pipeline_run: {
    method: "POST",
    path: (args) => `pipeline_runs/${escape(arg(args, "pipelineRunId"))}/resume`,
    body: (args) => ({
      reason: argOpt(args, "overrideReason") ?? null,
      idempotency_key: argOpt(args, "idempotencyKey") ?? null,
    }),
  },
  retry_pipeline_member: {
    method: "POST",
    path: (args) =>
      `pipeline_runs/${escape(arg(args, "pipelineRunId"))}/members/${escape(arg(args, "memberKey"))}/retry`,
    body: (args) => ({
      parameters: argOpt(args, "parameters") ?? {},
      override_reason: argOpt(args, "overrideReason") ?? null,
      idempotency_key: argOpt(args, "idempotencyKey") ?? null,
    }),
  },
  resolve_pipeline_run: {
    method: "POST",
    path: (args) => `pipeline_runs/${escape(arg(args, "pipelineRunId"))}/resolve`,
    body: (args) => ({
      decision: arg(args, "decision"),
      resolved_by: argOpt(args, "resolvedBy") ?? null,
      message: argOpt(args, "message") ?? null,
    }),
  },
  fetch_workflow_continuations: {
    method: "GET",
    path: (args) => `workflow_runs/${escape(arg(args, "workflowRunId"))}/continuations`,
  },
  fetch_workflow_effects: {
    method: "GET",
    path: (args) => `workflow_runs/${escape(arg(args, "workflowRunId"))}/effects`,
  },
  fetch_workflow_effect_output: {
    method: "GET",
    path: (args) => `workflow_effects/${escape(arg(args, "effectId"))}/output`,
  },
  control_workflow_effect_terminal: {
    method: "POST",
    path: (args) => `workflow_effects/${escape(arg(args, "effectId"))}/terminal`,
    body: (args) => arg(args, "control"),
  },
  settle_workflow_effect: {
    method: "POST",
    path: (args) => `workflow_effects/${escape(arg(args, "effectId"))}/settle`,
    body: (args) => ({
      status: arg(args, "status"),
      output: argOpt(args, "output") ?? null,
      message: argOpt(args, "message") ?? null,
    }),
  },
  fetch_workflow_journal: {
    method: "GET",
    path: (args) => `workflow_runs/${escape(arg(args, "workflowRunId"))}/journal`,
  },
  fetch_workflow_vm_cursors: {
    method: "GET",
    path: (args) => `workflow_runs/${escape(arg(args, "workflowRunId"))}/cursors`,
  },
  fetch_workflow_run_transitions: {
    method: "GET",
    path: (args) => `workflow_runs/${escape(arg(args, "workflowRunId"))}/transitions`,
  },
  fetch_workflow_node_transitions: {
    method: "GET",
    path: (args) =>
      `workflows/${escape(arg(args, "workflowId"))}/nodes/${escape(arg(args, "nodeId"))}/transitions`,
  },
  create_workflow_run: {
    method: "POST",
    path: (args) => `workflows/${escape(arg(args, "workflowId"))}/runs`,
    body: (args) => ({
      debug: argOpt(args, "debug") ?? false,
      parameters: argOpt(args, "parameters") ?? {},
      file_ids: argOpt(args, "fileIds") ?? [],
    }),
    transform: extractWorkflowRunId,
  },
  step_workflow_run: workflowRunDebugAction("step"),
  continue_workflow_run: workflowRunDebugAction("continue"),
  set_workflow_run_breakpoints: {
    method: "POST",
    path: (args) => `workflow_runs/${escape(arg(args, "workflowRunId"))}/debug/command`,
    body: (args) => ({
      verb: "set_breakpoints",
      breakpoints: arg(args, "breakpoints"),
    }),
  },
  run_workflow_to_node: {
    method: "POST",
    path: (args) => `workflow_runs/${escape(arg(args, "workflowRunId"))}/debug/command`,
    body: (args) => ({
      verb: "run_to",
      cursor: arg(args, "cursor"),
      node_id: arg(args, "nodeId"),
    }),
  },
  set_workflow_run_pause_on_failure: {
    method: "POST",
    path: (args) => `workflow_runs/${escape(arg(args, "workflowRunId"))}/debug/command`,
    body: (args) => ({ verb: "set_pause_on_failure", enabled: arg(args, "enabled") }),
  },
  cancel_workflow_run: {
    ...workflowRunAction("cancel"),
    body: (args) => ({
      reason: argOpt(args, "overrideReason") ?? null,
      idempotency_key: argOpt(args, "idempotencyKey") ?? null,
    }),
  },
  pause_workflow_run: {
    ...workflowRunAction("pause"),
    body: (args) => ({
      reason: argOpt(args, "overrideReason") ?? null,
      idempotency_key: argOpt(args, "idempotencyKey") ?? null,
    }),
  },
  resume_workflow_run: {
    ...workflowRunAction("resume"),
    body: (args) => ({
      reason: argOpt(args, "overrideReason") ?? null,
      idempotency_key: argOpt(args, "idempotencyKey") ?? null,
    }),
  },
  // every field is optional end to end (`#[serde(default)]`): an omitted source defaults to
  // `external`, and an omitted cursor targets whichever real thread drives next.
  request_run_interrupt: {
    method: "POST",
    path: (args) => `workflow_runs/${escape(arg(args, "workflowRunId"))}/interrupts`,
    body: (args) => ({
      source: argOpt(args, "source") ?? null,
      payload: argOpt(args, "payload") ?? null,
      continuation_id: argOpt(args, "continuationId") ?? null,
    }),
  },
  // signals had a tauri command but no http descriptor, so delivering one from the web build threw
  // "unknown command in web mode".
  deliver_signal: {
    method: "POST",
    path: (args) => `workflow_runs/${escape(arg(args, "workflowRunId"))}/signals`,
    body: (args) => ({ name: arg(args, "name"), payload: argOpt(args, "payload") ?? {} }),
  },
  fetch_supervisor_status: {
    method: "GET",
    path: () => "supervisor/status",
    accept404: true,
  },
  replay_workflow_run: {
    method: "POST",
    path: (args) => `workflow_runs/${escape(arg(args, "workflowRunId"))}/replay`,
    body: (args) => ({
      from_step_id: argOpt(args, "fromStepId") ?? null,
      override_reason: argOpt(args, "overrideReason") ?? null,
      idempotency_key: argOpt(args, "idempotencyKey") ?? null,
    }),
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
      const body = raw as { run?: unknown; nodes?: unknown; execution_state?: unknown };

      if (body.run == null) {
        throw new Error("missing workflow run");
      }

      return {
        run: body.run,
        nodes: body.nodes ?? [],
        execution_state: body.execution_state,
      };
    },
  },
  delete_workflow_run: {
    method: "DELETE",
    path: (args) => `workflow_runs/${escape(arg(args, "workflowRunId"))}`,
  },
  // packaged functions.
  list_function_packages: { method: "GET", path: () => "functions" },
  fetch_function_package: {
    method: "GET",
    path: (args) => `functions/${escape(arg(args, "packageName"))}`,
  },
  fetch_function_catalog: { method: "GET", path: () => "functions/catalog" },
  delete_function_package: {
    method: "DELETE",
    path: (args) => `functions/${escape(arg(args, "packageName"))}`,
  },
  set_function_alias: {
    method: "POST",
    path: (args) => `functions/${escape(arg(args, "packageName"))}/aliases`,
    body: (args) => ({
      alias: arg(args, "alias"),
      version: args?.version ?? null,
      from_alias: args?.fromAlias ?? null,
    }),
  },
  delete_function_alias: {
    method: "DELETE",
    path: (args) =>
      `functions/${escape(arg(args, "packageName"))}/aliases/${escape(arg(args, "alias"))}`,
  },
  invoke_function: {
    method: "POST",
    path: (args) => {
      const base = `functions/${escape(arg(args, "packageName"))}/${escape(arg(args, "exportName"))}/invocations`;
      const alias = argOpt(args, "alias");

      if (typeof alias === "string" && alias) {
        return `${base}?alias=${escape(alias)}`;
      }

      const version = argOpt(args, "version");
      // An alias wins over a version. Both identify the same package, and
      // sending both would leave the server to break the tie.
      return version == null ? base : `${base}?version=${escape(version)}`;
    },
    body: (args) => arg(args, "input"),
  },

  // the rexrap console.
  list_console_sessions: { method: "GET", path: () => "console/sessions" },
  create_console_session: {
    method: "POST",
    path: () => "console/sessions",
    body: (args) => ({ name: args?.name ?? null }),
  },
  fetch_console_session: {
    method: "GET",
    path: (args) => `console/sessions/${escape(arg(args, "sessionId"))}`,
  },
  rename_console_session: {
    method: "PATCH",
    path: (args) => `console/sessions/${escape(arg(args, "sessionId"))}`,
    body: (args) => ({ name: arg(args, "name") }),
  },
  delete_console_session: {
    method: "DELETE",
    path: (args) => `console/sessions/${escape(arg(args, "sessionId"))}`,
  },
  clear_console_session: {
    method: "POST",
    path: (args) => `console/sessions/${escape(arg(args, "sessionId"))}/clear`,
  },
  create_console_cell: {
    method: "POST",
    path: (args) => `console/sessions/${escape(arg(args, "sessionId"))}/cells`,
    body: (args) => ({
      source: arg(args, "source"),
      label: args?.label ?? null,
      position: args?.position ?? null,
    }),
  },
  fetch_console_cell: {
    method: "GET",
    path: (args) => `console/cells/${escape(arg(args, "cellId"))}`,
  },
  update_console_cell: {
    method: "PATCH",
    path: (args) => `console/cells/${escape(arg(args, "cellId"))}`,
    body: (args) => ({
      source: arg(args, "source"),
      label: args?.label ?? null,
      position: args?.position ?? null,
    }),
  },
  delete_console_cell: {
    method: "DELETE",
    path: (args) => `console/cells/${escape(arg(args, "cellId"))}`,
  },
  run_console_cell: {
    method: "POST",
    path: (args) => `console/cells/${escape(arg(args, "cellId"))}/run`,
  },
  cancel_console_cell: {
    method: "POST",
    path: (args) => `console/cells/${escape(arg(args, "cellId"))}/cancel`,
  },
  replay_console_cell: {
    method: "POST",
    path: (args) => `console/cells/${escape(arg(args, "cellId"))}/replay`,
  },

  fetch_resource_records: {
    method: "GET",
    path: (args) => String(arg(args, "endpoint")),
  },
  complete_rexrap: {
    method: "POST",
    path: () => "rexrap/complete",
    body: (args) => arg(args, "request"),
  },
  hover_rexrap: {
    method: "POST",
    path: () => "rexrap/hover",
    body: (args) => arg(args, "request"),
  },
  compile_rexrap: {
    method: "POST",
    path: () => "rexrap/compile",
    body: (args) => ({ source: arg(args, "source"), enabled: arg(args, "enabled") }),
  },
  analyze_rexrap: {
    method: "POST",
    path: () => "rexrap/analyze",
    body: (args) => ({
      source: arg(args, "source"),
      source_path: argOpt(args, "sourcePath") ?? null,
    }),
  },
  format_rexrap: {
    method: "POST",
    path: () => "rexrap/format",
    body: (args) => ({ source: arg(args, "source") }),
  },
  decompile_to_rexrap: {
    method: "POST",
    path: () => "rexrap/decompile",
    body: (args) => ({ workflow: arg(args, "workflow") }),
  },
  evaluate_expression: {
    method: "POST",
    path: () => "rexrap/evaluate",
    body: (args) => ({ expression: arg(args, "expression"), context: arg(args, "context") }),
  },
  fetch_providers: { method: "GET", path: () => "providers" },
  fetch_node_kinds: { method: "GET", path: () => "node-kinds" },
  fetch_trigger_kinds: { method: "GET", path: () => "trigger-kinds" },
  fetch_enum_catalogs: { method: "GET", path: () => "catalog/enums" },
  fetch_replicas: { method: "GET", path: () => "replicas" },
  kick_replica: {
    method: "POST",
    path: (args) => `replicas/${escape(arg(args, "replicaId"))}/kick`,
  },
  create_agent_directive: {
    method: "POST",
    path: (args) => `replicas/${escape(arg(args, "replicaId"))}/directives`,
    body: (args) => ({ kind: arg(args, "kind"), expires_in_seconds: 300 }),
  },
  list_agent_directives: {
    method: "GET",
    path: (args) =>
      `replicas/${escape(arg(args, "replicaId"))}/directives?limit=${escape(arg(args, "limit"))}`,
  },
  create_agent_enrollment_token: {
    method: "POST",
    path: () => "agents/enrollment_tokens",
    body: (args) => arg(args, "request"),
  },
  list_agent_enrollment_tokens: { method: "GET", path: () => "agents/enrollment_tokens" },
  revoke_agent_enrollment_token: {
    method: "DELETE",
    path: (args) => `agents/enrollment_tokens/${escape(arg(args, "tokenId"))}`,
  },
  list_agent_machines: { method: "GET", path: () => "agents/machines" },
  invalidate_agent_machine: {
    method: "DELETE",
    path: (args) => `agents/machines/${escape(arg(args, "machineId"))}`,
  },
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
  move_credential: {
    method: "PATCH",
    path: (args) => `credentials/${escape(arg(args, "settingId"))}`,
    body: (args) => arg(args, "request"),
  },
  fetch_approvals: {
    method: "GET",
    path: (args) => {
      const workflowRunId = argOpt(args, "workflowRunId");
      return workflowRunId == null
        ? "approvals"
        : `approvals?workflow_run_id=${escape(workflowRunId)}`;
    },
  },
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
  fetch_notification_deliveries: {
    method: "GET",
    path: (args) => `notifications/${escape(arg(args, "notificationId"))}/deliveries`,
  },
  fetch_notification_policies: {
    method: "GET",
    path: (args) => {
      const workflowId = argOpt(args, "workflowId");
      return workflowId
        ? `notification_policies?workflow_id=${escape(workflowId)}`
        : "notification_policies";
    },
  },
  create_notification_policy: {
    method: "POST",
    path: () => "notification_policies",
    body: (args) => arg(args, "policy"),
  },
  update_notification_policy: {
    method: "PATCH",
    path: (args) => `notification_policies/${escape(arg(args, "policyId"))}`,
    body: (args) => arg(args, "policy"),
  },
  delete_notification_policy: {
    method: "DELETE",
    path: (args) => `notification_policies/${escape(arg(args, "policyId"))}`,
  },
  fetch_freeze_windows: {
    method: "GET",
    path: (args) => (argOpt(args, "activeOnly") ? "freeze_windows?active=true" : "freeze_windows"),
  },
  create_freeze_window: {
    method: "POST",
    path: () => "freeze_windows",
    body: (args) => arg(args, "window"),
  },
  update_freeze_window: {
    method: "PATCH",
    path: (args) => `freeze_windows/${escape(arg(args, "windowId"))}`,
    body: (args) => arg(args, "window"),
  },
  delete_freeze_window: {
    method: "DELETE",
    path: (args) => `freeze_windows/${escape(arg(args, "windowId"))}`,
  },
  backfill_workflow_trigger: {
    method: "POST",
    path: (args) => `workflow_triggers/${escape(arg(args, "triggerId"))}/backfill`,
    body: (args) => arg(args, "request"),
  },
  create_calendar_subscription: {
    method: "POST",
    path: () => "schedules/calendar-subscriptions",
    body: (args) => ({ scope: arg(args, "scope"), org_id: argOpt(args, "orgId") ?? null }),
  },
  delete_calendar_subscription: {
    method: "DELETE",
    path: (args) => `schedules/calendar-subscriptions/${escape(arg(args, "subscriptionId"))}`,
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
  import_pack_archive: {
    method: "POST",
    path: (args) =>
      `packs/import?overwrite=${argOpt(args, "overwrite") === true ? "true" : "false"}`,
    rawBody: (args) => ({
      body: arg(args, "bytes") as ArrayBuffer,
      contentType: "application/zip",
    }),
  },
  list_workflow_files: { method: "GET", path: () => "workflow_files" },
  list_execution_profiles: { method: "GET", path: () => "execution_profiles" },
  list_execution_profile_collection_statuses: {
    method: "GET",
    path: () => "execution_profiles/collection-statuses",
  },
  put_execution_profile: {
    method: "PUT",
    path: (args) => `execution_profiles/${escape(arg(args, "profileId"))}`,
    body: (args) => arg(args, "profile"),
  },
  delete_execution_profile: {
    method: "DELETE",
    path: (args) => `execution_profiles/${escape(arg(args, "profileId"))}`,
  },
  rotate_execution_profile: {
    method: "POST",
    path: (args) => `execution_profiles/${escape(arg(args, "profileId"))}/rotate`,
  },
  test_execution_profile: {
    method: "POST",
    path: (args) => `execution_profiles/${escape(arg(args, "profileId"))}/test`,
  },
  upload_workflow_file: {
    method: "POST",
    path: (args) => {
      const query = new URLSearchParams({ path: String(arg(args, "path")) });
      const mimeType = argOpt(args, "mimeType");

      if (typeof mimeType === "string" && mimeType) {
        query.set("mime_type", mimeType);
      }

      return `workflow_files?${query.toString()}`;
    },
    rawBody: (args) => {
      const mimeType = argOpt(args, "mimeType");

      return {
        body: arg(args, "bytes") as ArrayBuffer,
        contentType: typeof mimeType === "string" ? mimeType : "application/octet-stream",
      };
    },
  },
  stage_workflow_file: {
    method: "POST",
    path: (args) => {
      const query = new URLSearchParams({ path: String(arg(args, "path")) });
      const mimeType = argOpt(args, "mimeType");

      if (typeof mimeType === "string" && mimeType) {
        query.set("mime_type", mimeType);
      }

      return `workflow_files/stage?${query.toString()}`;
    },
    rawBody: (args) => {
      const mimeType = argOpt(args, "mimeType");

      return {
        body: arg(args, "bytes") as ArrayBuffer,
        contentType: typeof mimeType === "string" ? mimeType : "application/octet-stream",
      };
    },
  },
  archive_workflow_file: {
    method: "DELETE",
    path: (args) => `workflow_files/${escape(arg(args, "fileId"))}`,
  },
  // packaged function archives: bytes under their own digest, stored only if absent.
  upload_function_artifact: {
    method: "POST",
    path: (args) => `function_artifacts/${escape(arg(args, "digest"))}`,
    rawBody: (args) => ({
      body: arg(args, "bytes") as ArrayBuffer,
      contentType: "application/zip",
    }),
  },
  publish_function_version: {
    method: "POST",
    path: () => "functions",
    body: (args) => arg(args, "request"),
  },
  restore_function_package: {
    method: "POST",
    path: (args) => `functions/${escape(arg(args, "packageName"))}/restore`,
  },
  fetch_replica_providers: {
    method: "GET",
    path: (args) => `replicas/${escape(arg(args, "replicaId"))}/providers`,
  },
  fetch_replica_samples: {
    method: "GET",
    path: (args) => {
      const base = `replicas/${escape(arg(args, "replicaId"))}/samples`;
      const since = argOpt(args, "sinceSeconds");
      return since ? `${base}?since_seconds=${escape(since)}` : base;
    },
  },
  // --- organizations (tenants), membership, resource allocation, and billing ---
  list_my_orgs: { method: "GET", path: () => "orgs/me" },
  list_orgs: { method: "GET", path: () => "orgs" },
  create_org: {
    method: "POST",
    path: () => "orgs",
    body: (args) => ({ name: arg(args, "name") }),
  },
  update_org: {
    method: "PATCH",
    path: (args) => `orgs/${escape(arg(args, "orgId"))}`,
    headers: (args) => ({ "x-org-id": String(arg(args, "orgId")) }),
    body: (args) => ({ name: arg(args, "name") }),
  },
  switch_org: {
    method: "POST",
    path: () => "auth/switch-org",
    body: (args) => ({ org_id: arg(args, "orgId") }),
  },
  switch_platform: { method: "POST", path: () => "auth/switch-platform" },
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
    path: (args) => `orgs/${escape(arg(args, "orgId"))}/members/${escape(arg(args, "userId"))}`,
    headers: (args) => ({ "x-org-id": String(arg(args, "orgId")) }),
    body: (args) => ({ role: arg(args, "role") }),
  },
  remove_org_member: {
    method: "DELETE",
    path: (args) => `orgs/${escape(arg(args, "orgId"))}/members/${escape(arg(args, "userId"))}`,
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
    // the cursor is optional end to end: omitted, the backend targets whichever branch is parked,
    // which is what keeps a single-cursor client's payloads working against a forked run.
    body: (args) => ({ cursor: argOpt(args, "cursor") ?? null }),
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

  if (descriptor.rawBody) {
    const raw = descriptor.rawBody(args);
    headers["content-type"] = raw.contentType;
    init.body = raw.body;
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
  if (name === "save_workflow_bundle" || name === "save_workflow_rexrap") {
    const saved =
      (name === "save_workflow_bundle"
        ? (raw as { workflows?: { workflows?: { id?: string | null }[] } }).workflows
        : (raw as { workflows?: { id?: string | null }[] })) ?? {};
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
