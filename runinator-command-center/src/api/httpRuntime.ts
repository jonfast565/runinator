// Web-mode runtime adapter. Translates Tauri command names + args into HTTP
// requests against runinator-ws. Used when the SPA runs outside of Tauri
// (browser, in-cluster deployment).
//
// Path conventions: all paths are relative to apiBaseUrl(). In production the
// SPA is served by an nginx pod that reverse-proxies "/api/*" to runinator-ws,
// so apiBaseUrl() returns "/api". In `vite dev` we either rely on the dev
// server proxy (default) or honor VITE_RUNINATOR_WS_URL for direct override.

type Method = "GET" | "POST" | "PATCH" | "DELETE";

type CommandArgs = Record<string, unknown> | undefined;

type HttpDescriptor = {
  method: Method | ((args: CommandArgs) => Method);
  path: (args: CommandArgs) => string;
  body?: (args: CommandArgs) => unknown;
  transform?: (raw: unknown) => unknown;
  accept404?: boolean;
};

function arg<T = unknown>(args: CommandArgs, key: string): T {
  if (!args || !(key in args)) {
    throw new Error(`Missing argument '${key}'`);
  }
  return args[key] as T;
}

function argOpt<T = unknown>(args: CommandArgs, key: string): T | undefined {
  if (!args) return undefined;
  return args[key] as T | undefined;
}

function escape(part: string | number): string {
  return encodeURIComponent(String(part));
}

const REGISTRY: Record<string, HttpDescriptor> = {
  fetch_workflows: { method: "GET", path: () => "workflows" },
  save_workflow: {
    method: (args) => (arg<{ id?: number | null }>(args, "workflow").id != null ? "PATCH" : "POST"),
    path: (args) => {
      const workflow = arg<{ id?: number | null }>(args, "workflow");
      return workflow.id != null ? `workflows/${escape(workflow.id)}` : "workflows";
    },
    body: (args) => arg(args, "workflow")
  },
  save_workflow_bundle: {
    method: "POST",
    path: () => "workflows/import",
    body: (args) => arg(args, "request")
  },
  delete_workflow: {
    method: "DELETE",
    path: (args) => `workflows/${escape(arg<number>(args, "workflowId"))}`
  },
  fetch_workflow_triggers: {
    method: "GET",
    path: (args) => `workflows/${escape(arg<number>(args, "workflowId"))}/triggers`
  },
  save_workflow_trigger: {
    method: (args) => (arg<boolean>(args, "creating") ? "POST" : "PATCH"),
    path: (args) => {
      const creating = arg<boolean>(args, "creating");
      const trigger = arg<{ id?: number | null; workflow_id: number }>(args, "trigger");
      if (creating) return `workflows/${escape(trigger.workflow_id)}/triggers`;
      if (trigger.id == null) throw new Error("missing workflow trigger id");
      return `workflow_triggers/${escape(trigger.id)}`;
    },
    body: (args) => arg(args, "trigger")
  },
  delete_workflow_trigger: {
    method: "DELETE",
    path: (args) => `workflow_triggers/${escape(arg<number>(args, "triggerId"))}`
  },
  fetch_run_chunks: {
    method: "GET",
    path: (args) => `runs/${escape(arg<number>(args, "runId"))}/chunks?limit=500`
  },
  fetch_run_artifacts: {
    method: "GET",
    path: (args) => `runs/${escape(arg<number>(args, "runId"))}/artifacts`
  },
  fetch_workflow_node_run_chunks: {
    method: "GET",
    path: (args) => `workflow_node_runs/${escape(arg<number>(args, "nodeRunId"))}/chunks?limit=500`
  },
  fetch_workflow_node_run_artifacts: {
    method: "GET",
    path: (args) => `workflow_node_runs/${escape(arg<number>(args, "nodeRunId"))}/artifacts`
  },
  create_workflow_run: {
    method: "POST",
    path: (args) => `workflows/${escape(arg<number>(args, "workflowId"))}/runs`,
    body: (args) => ({ debug: argOpt<boolean>(args, "debug") ?? false }),
    transform: extractWorkflowRunId
  },
  step_workflow_run: workflowRunDebugAction("step"),
  continue_workflow_run: workflowRunDebugAction("continue"),
  cancel_workflow_run: workflowRunAction("cancel"),
  pause_workflow_run: workflowRunAction("pause"),
  resume_workflow_run: workflowRunAction("resume"),
  patch_workflow_run_debug: {
    method: "PATCH",
    path: (args) => `workflow_runs/${escape(arg<number>(args, "workflowRunId"))}/debug`,
    body: (args) => arg(args, "patch")
  },
  run_to_cursor_workflow_run: {
    method: "POST",
    path: (args) => `workflow_runs/${escape(arg<number>(args, "workflowRunId"))}/debug/run_to_cursor`,
    body: (args) => ({ node_id: arg(args, "nodeId") })
  },
  skip_workflow_node: {
    method: "POST",
    path: (args) => `workflow_runs/${escape(arg<number>(args, "workflowRunId"))}/debug/skip`,
    body: (args) => ({
      output_json: arg(args, "outputJson"),
      message: argOpt(args, "message") ?? null
    })
  },
  rerun_workflow_node: {
    method: "POST",
    path: (args) => `workflow_runs/${escape(arg<number>(args, "workflowRunId"))}/debug/rerun_node`,
    body: (args) => ({ parameters: arg(args, "parameters") })
  },
  fetch_supervisor_status: {
    method: "GET",
    path: () => "supervisor/status",
    accept404: true
  },
  replay_workflow_run: {
    method: "POST",
    path: (args) => `workflow_runs/${escape(arg<number>(args, "workflowRunId"))}/replay`,
    body: (args) => ({ from_step_id: argOpt(args, "fromStepId") ?? null }),
    transform: extractWorkflowRunId
  },
  rename_workflow_run: {
    method: "POST",
    path: (args) => `workflow_runs/${escape(arg<number>(args, "workflowRunId"))}/rename`,
    body: (args) => ({ name: argOpt(args, "name") ?? null })
  },
  fetch_workflow_runs: {
    method: "GET",
    path: (args) => {
      const workflowId = argOpt<number | null>(args, "workflowId");
      return workflowId != null ? `workflow_runs?workflow_id=${escape(workflowId)}` : "workflow_runs";
    }
  },
  fetch_workflow_run: {
    method: "GET",
    path: (args) => `workflow_runs/${escape(arg<number>(args, "workflowRunId"))}`,
    transform: (raw) => {
      const body = raw as { run?: unknown; nodes?: unknown };
      if (!body || typeof body !== "object" || body.run == null) {
        throw new Error("missing workflow run");
      }
      return { run: body.run, nodes: body.nodes ?? [] };
    }
  },
  fetch_resource_records: {
    method: "GET",
    path: (args) => arg<string>(args, "endpoint")
  },
  fetch_providers: { method: "GET", path: () => "providers" },
  fetch_credentials: { method: "GET", path: () => "credentials" },
  save_credential: {
    method: "POST",
    path: () => "credentials",
    body: (args) => arg(args, "request")
  },
  delete_credential: {
    method: "DELETE",
    path: (args) =>
      `credentials?scope=${escape(arg<string>(args, "scope"))}&name=${escape(arg<string>(args, "name"))}`
  },
  approve_approval: {
    method: "POST",
    path: (args) => `approvals/${escape(arg<number>(args, "approvalId"))}/approve`,
    body: () => ({})
  },
  reject_approval: {
    method: "POST",
    path: (args) => `approvals/${escape(arg<number>(args, "approvalId"))}/reject`,
    body: () => ({})
  },
  fetch_all_artifacts: { method: "GET", path: () => "artifacts" },
  fetch_notifications: {
    method: "GET",
    path: (args) => {
      const limit = (argOpt<number>(args, "limit") ?? 200) as number;
      const unread = Boolean(argOpt(args, "unreadOnly"));
      const base = `notifications?limit=${escape(limit)}`;
      return unread ? `${base}&unread=true` : base;
    }
  },
  mark_notification_read: {
    method: "POST",
    path: (args) => `notifications/${escape(arg<number>(args, "notificationId"))}/mark_read`,
    body: () => ({})
  },
  mark_all_notifications_read: {
    method: "POST",
    path: () => "notifications/mark_all_read",
    body: () => ({})
  }
};

function workflowRunAction(action: string): HttpDescriptor {
  return {
    method: "POST",
    path: (args) => `workflow_runs/${escape(arg<number>(args, "workflowRunId"))}/${action}`,
    body: () => ({})
  };
}

function workflowRunDebugAction(action: string): HttpDescriptor {
  return {
    method: "POST",
    path: (args) => `workflow_runs/${escape(arg<number>(args, "workflowRunId"))}/debug/${action}`,
    body: () => ({})
  };
}

function extractWorkflowRunId(raw: unknown): { id: number } {
  const body = raw as { run?: { id?: number } } | null;
  const id = body?.run?.id;
  if (typeof id !== "number") throw new Error("missing workflow run id");
  return { id };
}

export function apiBaseUrl(): string {
  const override = (import.meta as { env?: Record<string, string | undefined> }).env?.VITE_RUNINATOR_WS_URL;
  if (override && override.trim().length > 0) {
    return override.replace(/\/+$/, "");
  }
  return "/api";
}

export function wsBaseUrl(): string {
  const override = (import.meta as { env?: Record<string, string | undefined> }).env?.VITE_RUNINATOR_WS_URL;
  if (override && override.trim().length > 0) {
    return override.replace(/\/+$/, "");
  }
  if (typeof window === "undefined") return "";
  return window.location.origin;
}

export async function invokeViaHttp<T>(name: string, args?: Record<string, unknown>): Promise<T> {
  if (name === "get_service_status") {
    return { service_url: wsBaseUrl() || null } as unknown as T;
  }
  if (name === "start_service_discovery") {
    return undefined as unknown as T;
  }
  if (name === "upload_artifact" || name === "download_artifact") {
    throw new Error(`${name} is not available in web mode; use uploadArtifactBlob/downloadArtifactBlob instead`);
  }

  const descriptor = REGISTRY[name];
  if (!descriptor) {
    throw new Error(`Unknown command in web mode: ${name}`);
  }

  const base = apiBaseUrl();
  const path = descriptor.path(args).replace(/^\/+/, "");
  const url = `${base}/${path}`;
  const method = typeof descriptor.method === "function" ? descriptor.method(args) : descriptor.method;
  const init: RequestInit = { method };
  if (descriptor.body) {
    init.headers = { "content-type": "application/json" };
    init.body = JSON.stringify(descriptor.body(args));
  }

  const response = await fetch(url, init);
  if (response.status === 404 && descriptor.accept404) {
    return (await response.json()) as T;
  }
  if (!response.ok) {
    const text = await response.text().catch(() => "");
    throw new Error(`${method} ${url} -> ${response.status}: ${text}`);
  }
  if (response.status === 204) return undefined as unknown as T;
  const raw = await response.json();

  // save_workflow_bundle: after import, re-export the first saved workflow to
  // hydrate the bundle with server-assigned ids — mirrors the Tauri command.
  if (name === "save_workflow_bundle") {
    const saved = raw as { workflows?: Array<{ id?: number | null }> };
    const id = saved.workflows?.[0]?.id;
    if (id == null) return saved as unknown as T;
    const exportResp = await fetch(`${base}/workflows/${escape(id)}/export`);
    if (!exportResp.ok) {
      const text = await exportResp.text().catch(() => "");
      throw new Error(`GET workflows/${id}/export -> ${exportResp.status}: ${text}`);
    }
    return (await exportResp.json()) as T;
  }

  return (descriptor.transform ? descriptor.transform(raw) : raw) as T;
}
