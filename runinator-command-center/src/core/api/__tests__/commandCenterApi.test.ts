import { beforeEach, describe, expect, it, vi } from "vitest";
import { setCommandRuntime } from "../runtime";
import { apiBaseUrl, invokeViaHttp, wsBaseUrl } from "../httpRuntime";
import {
  addOrchestrationAlias,
  addTeamMember,
  cancelPipelineRun,
  cancelWorkflowRun,
  clearConsoleSession,
  createApiKey,
  createUser,
  deleteOrchestrationAlias,
  deliverSignal,
  fetchEnumCatalogs,
  fetchNodeKinds,
  fetchOrchestrationAliases,
  fetchTriggerKinds,
  fetchWorkflowRun,
  importPackArchive,
  listTeamMembers,
  requestRunInterrupt,
  rotateApiKey,
  saveWorkflowBundle,
  updateApiKey,
  updateTeam,
} from "../commandCenterApi";
import { invoke } from "@tauri-apps/api/core";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

describe("command center catalog metadata API", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockResolvedValue([]);
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
    setCommandRuntime({
      isTauri: () => true,
      invoke: (name, args) => invoke(name, args),
      wsBaseUrl: () => "http://127.0.0.1:8080",
      apiBaseUrl: () => "/api",
    });
  });

  it("requests node kinds", async () => {
    await fetchNodeKinds();
    expect(invoke).toHaveBeenCalledWith("fetch_node_kinds", undefined);
  });

  it("requests trigger kinds", async () => {
    await fetchTriggerKinds();
    expect(invoke).toHaveBeenCalledWith("fetch_trigger_kinds", undefined);
  });

  it("requests enum catalogs", async () => {
    await fetchEnumCatalogs();
    expect(invoke).toHaveBeenCalledWith("fetch_enum_catalogs", undefined);
  });

  it("passes managed-run override audit fields through the shared command runtime", async () => {
    vi.mocked(invoke).mockResolvedValue({ success: true, message: "accepted" });

    await cancelPipelineRun("pipeline-run-1", {
      reason: "recover a wedged executor",
      idempotencyKey: "pipeline-override-1",
    });
    await cancelWorkflowRun("workflow-run-1", {
      reason: "contain a provider incident",
      idempotencyKey: "workflow-override-1",
    });

    expect(invoke).toHaveBeenCalledWith("cancel_pipeline_run", {
      pipelineRunId: "pipeline-run-1",
      overrideReason: "recover a wedged executor",
      idempotencyKey: "pipeline-override-1",
    });
    expect(invoke).toHaveBeenCalledWith("cancel_workflow_run", {
      workflowRunId: "workflow-run-1",
      overrideReason: "contain a provider incident",
      idempotencyKey: "workflow-override-1",
    });
  });

  it("keeps completed VM effects attached to the node that issued them", async () => {
    vi.mocked(invoke).mockImplementation((name) => {
      const responses: Record<string, unknown> = {
        fetch_workflow_run: {
          run: {
            id: "run-1",
            workflow_id: "workflow-1",
            status: "succeeded",
            active_node_id: "end",
            created_at: "",
            started_at: null,
            finished_at: "",
          },
          nodes: [],
        },
        fetch_workflow_continuations: [],
        fetch_workflow_effects: [
          {
            version: 1,
            id: "effect-1",
            workflow_run_id: "run-1",
            continuation_id: "continuation-1",
            sequence: 0,
            attempt: 0,
            node_id: "publish",
            request: { type: "action" },
            status: "succeeded",
            created_at: 0,
            updated_at: 0,
            finished_at: 1,
          },
        ],
        fetch_workflow_journal: [
          {
            version: 1,
            id: "journal-1",
            workflow_run_id: "run-1",
            sequence: 1,
            continuation_id: "continuation-1",
            entry: { type: "node_entered", continuation_id: "continuation-1", node_id: "config" },
            created_at: 0,
          },
          {
            version: 1,
            id: "journal-2",
            workflow_run_id: "run-1",
            sequence: 2,
            continuation_id: "continuation-1",
            entry: { type: "node_entered", continuation_id: "continuation-1", node_id: "publish" },
            created_at: 1,
          },
        ],
        // The continuation has moved to end. This used to overwrite the effect's true node.
        fetch_workflow_vm_cursors: [
          {
            continuation_id: "continuation-1",
            instruction_pointer: 99,
            node_id: "end",
            status: "succeeded",
          },
        ],
      };
      return Promise.resolve(responses[name]);
    });

    const detail = await fetchWorkflowRun("run-1");

    expect(detail.nodes).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ node_id: "config", status: "succeeded" }),
        expect.objectContaining({ node_id: "publish", status: "succeeded" }),
      ]),
    );
  });

  it("keeps the server's materialized steps when VM history is unavailable", async () => {
    vi.mocked(invoke).mockImplementation((name) => {
      const responses: Record<string, unknown> = {
        fetch_workflow_run: {
          run: { id: "run-1", workflow_id: "workflow-1", status: "failed" },
          nodes: [
            {
              id: "node-run-1",
              workflow_run_id: "run-1",
              node_id: "mutex_1",
              status: "timed_out",
              attempt: 0,
              parameters: {},
              created_at: "2026-08-29T03:00:00.000Z",
              started_at: "2026-08-29T03:00:00.000Z",
              finished_at: "2026-08-29T03:05:00.000Z",
              message: "no result within 300s; the executing worker never reported",
            },
          ],
        },
        fetch_workflow_continuations: [],
        fetch_workflow_effects: new Error("temporarily unavailable"),
        fetch_workflow_journal: new Error("temporarily unavailable"),
        fetch_workflow_vm_cursors: new Error("temporarily unavailable"),
      };
      const response = responses[name];
      return response instanceof Error ? Promise.reject(response) : Promise.resolve(response);
    });

    const detail = await fetchWorkflowRun("run-1");

    expect(detail.nodes).toEqual([
      expect.objectContaining({
        id: "node-run-1",
        node_id: "mutex_1",
        status: "timed_out",
      }),
    ]);
  });

  it("merges VM action effects into a partially materialized step list", async () => {
    vi.mocked(invoke).mockImplementation((name) => {
      const responses: Record<string, unknown> = {
        fetch_workflow_run: {
          run: { id: "run-1", workflow_id: "workflow-1", status: "succeeded" },
          nodes: [
            {
              id: "legacy-mutex-row",
              workflow_run_id: "run-1",
              node_id: "mutex_1",
              status: "succeeded",
              attempt: 0,
              parameters: {},
              output_json: { acquired: true },
              // Legacy rows commonly use empty strings for timestamps. The VM effect is the
              // durable source and must replace those sentinels.
              created_at: "",
              started_at: "",
              finished_at: "",
              message: null,
            },
          ],
        },
        fetch_workflow_continuations: [],
        fetch_workflow_effects: [
          {
            version: 1,
            id: "effect-mutex",
            workflow_run_id: "run-1",
            continuation_id: "continuation-1",
            sequence: 0,
            attempt: 0,
            node_id: "mutex_1",
            request: { type: "coordination", kind: "mutex" },
            status: "succeeded",
            result: { acquired: true },
            created_at: 1787974992,
            updated_at: 1787974992,
            finished_at: 1787974992,
          },
          {
            version: 1,
            id: "effect-sync",
            workflow_run_id: "run-1",
            continuation_id: "continuation-1",
            sequence: 1,
            attempt: 0,
            node_id: "sync_claude",
            request: { type: "action", provider: "console", function: "run" },
            status: "succeeded",
            result: { exit_code: 0 },
            created_at: 1787974993,
            updated_at: 1787974995,
            finished_at: 1787974995,
          },
        ],
        fetch_workflow_journal: [],
        fetch_workflow_vm_cursors: [],
      };
      return Promise.resolve(responses[name]);
    });

    const detail = await fetchWorkflowRun("run-1");

    expect(detail.nodes).toHaveLength(2);
    expect(detail.nodes.find((node) => node.node_id === "mutex_1")).toMatchObject({
      created_at: "2026-08-29T03:43:12.000Z",
      started_at: "2026-08-29T03:43:12.000Z",
      finished_at: "2026-08-29T03:43:12.000Z",
    });
    expect(detail.nodes).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ id: "legacy-mutex-row", node_id: "mutex_1" }),
        expect.objectContaining({
          id: "effect-sync",
          node_id: "sync_claude",
          status: "succeeded",
          output_json: { exit_code: 0 },
        }),
      ]),
    );
  });

  it("keeps every projected node category when materialized steps are also present", async () => {
    const effects = [
      { id: "action", nodeId: "publish", status: "succeeded", request: { type: "action" } },
      { id: "approval", nodeId: "review", status: "requested", request: { type: "approval" } },
      { id: "input", nodeId: "collect_input", status: "running", request: { type: "input" } },
      { id: "signal", nodeId: "await_signal", status: "requested", request: { type: "signal" } },
      { id: "timer", nodeId: "backoff", status: "running", request: { type: "timer" } },
      {
        id: "mutex",
        nodeId: "serialize",
        status: "succeeded",
        request: { type: "coordination", kind: "mutex" },
      },
    ].map(({ id, nodeId, status, request }, sequence) => ({
      version: 1,
      id: `effect-${id}`,
      workflow_run_id: "run-1",
      continuation_id: "continuation-1",
      sequence,
      attempt: 0,
      node_id: nodeId,
      request,
      status,
      created_at: sequence + 1,
      updated_at: sequence + 1,
      finished_at: status === "succeeded" ? sequence + 2 : null,
    }));

    vi.mocked(invoke).mockImplementation((name) => {
      const responses: Record<string, unknown> = {
        fetch_workflow_run: {
          run: { id: "run-1", workflow_id: "workflow-1", status: "running" },
          nodes: [
            {
              id: "materialized-compute",
              workflow_run_id: "run-1",
              node_id: "prepare",
              status: "succeeded",
              attempt: 0,
              parameters: {},
              created_at: "1970-01-01T00:00:00.000Z",
              started_at: "1970-01-01T00:00:00.000Z",
              finished_at: "1970-01-01T00:00:00.001Z",
              message: null,
            },
          ],
        },
        fetch_workflow_continuations: [],
        fetch_workflow_effects: effects,
        fetch_workflow_journal: [],
        fetch_workflow_vm_cursors: [],
      };
      return Promise.resolve(responses[name]);
    });

    const detail = await fetchWorkflowRun("run-1");

    expect(detail.nodes.map((node) => node.node_id)).toEqual([
      "prepare",
      "publish",
      "review",
      "collect_input",
      "await_signal",
      "backoff",
      "serialize",
    ]);
    expect(
      Object.fromEntries(detail.nodes.map((node) => [node.node_id, node.status])),
    ).toMatchObject({
      prepare: "succeeded",
      publish: "succeeded",
      review: "approval_required",
      collect_input: "waiting",
      await_signal: "waiting",
      backoff: "running",
      serialize: "succeeded",
    });
  });

  it("projects an effect to the node entries persisted after its request boundary", async () => {
    vi.mocked(invoke).mockImplementation((name) => {
      const responses: Record<string, unknown> = {
        fetch_workflow_run: {
          run: { id: "run-1", workflow_id: "workflow-1", status: "succeeded" },
          nodes: [],
        },
        fetch_workflow_continuations: [],
        fetch_workflow_effects: [
          {
            version: 1,
            id: "effect-1",
            workflow_run_id: "run-1",
            continuation_id: "continuation-1",
            sequence: 0,
            attempt: 0,
            node_id: null,
            request: { type: "action" },
            status: "succeeded",
            created_at: 0,
            updated_at: 1,
            finished_at: 1,
          },
        ],
        // The database writes EffectRequested before draining the continuation's accumulated
        // NodeEntered records.  Once the effect settles, a later end-node entry must not take
        // ownership of the completed action.
        fetch_workflow_journal: [
          {
            version: 1,
            id: "requested",
            workflow_run_id: "run-1",
            sequence: 1,
            continuation_id: "continuation-1",
            entry: { type: "effect_requested", effect_id: "effect-1", instruction_pointer: 1 },
            created_at: 0,
          },
          {
            version: 1,
            id: "entered-start",
            workflow_run_id: "run-1",
            sequence: 2,
            continuation_id: "continuation-1",
            entry: { type: "node_entered", continuation_id: "continuation-1", node_id: "start" },
            created_at: 0,
          },
          {
            version: 1,
            id: "entered-greeting",
            workflow_run_id: "run-1",
            sequence: 3,
            continuation_id: "continuation-1",
            entry: { type: "node_entered", continuation_id: "continuation-1", node_id: "greeting" },
            created_at: 0,
          },
          {
            version: 1,
            id: "settled",
            workflow_run_id: "run-1",
            sequence: 4,
            continuation_id: "continuation-1",
            entry: { type: "effect_settled", effect_id: "effect-1", status: "succeeded" },
            created_at: 1,
          },
          {
            version: 1,
            id: "entered-end",
            workflow_run_id: "run-1",
            sequence: 5,
            continuation_id: "continuation-1",
            entry: { type: "node_entered", continuation_id: "continuation-1", node_id: "end" },
            created_at: 1,
          },
        ],
        fetch_workflow_vm_cursors: [
          {
            continuation_id: "continuation-1",
            instruction_pointer: 3,
            node_id: "end",
            status: "succeeded",
          },
        ],
      };
      return Promise.resolve(responses[name]);
    });

    const detail = await fetchWorkflowRun("run-1");

    expect(detail.nodes).toContainEqual(
      expect.objectContaining({ id: "effect-1", node_id: "greeting", status: "succeeded" }),
    );
  });

  it("marks an inline node failed when its journaled evaluation fails", async () => {
    vi.mocked(invoke).mockImplementation((name) => {
      const responses: Record<string, unknown> = {
        fetch_workflow_run: {
          run: { id: "run-1", workflow_id: "workflow-1", status: "failed" },
          nodes: [],
        },
        fetch_workflow_continuations: [],
        fetch_workflow_effects: [],
        fetch_workflow_journal: [
          {
            version: 1,
            id: "journal-1",
            workflow_run_id: "run-1",
            sequence: 1,
            continuation_id: "continuation-1",
            entry: { type: "node_entered", continuation_id: "continuation-1", node_id: "config" },
            created_at: 0,
          },
          {
            version: 1,
            id: "journal-2",
            workflow_run_id: "run-1",
            sequence: 2,
            continuation_id: "continuation-1",
            entry: { type: "failed", continuation_id: "continuation-1", node_id: "config" },
            created_at: 1,
          },
        ],
        fetch_workflow_vm_cursors: [],
      };
      return Promise.resolve(responses[name]);
    });

    const detail = await fetchWorkflowRun("run-1");

    expect(detail.nodes).toMatchObject([{ node_id: "config", status: "failed" }]);
  });

  it("keeps every scheduled retry in the projected node history", async () => {
    vi.mocked(invoke).mockImplementation((name) => {
      const responses: Record<string, unknown> = {
        fetch_workflow_run: {
          run: { id: "run-1", workflow_id: "workflow-1", status: "failed" },
          nodes: [],
        },
        fetch_workflow_continuations: [],
        fetch_workflow_effects: [
          {
            version: 1,
            id: "effect-1",
            workflow_run_id: "run-1",
            continuation_id: "continuation-1",
            sequence: 0,
            attempt: 3,
            // Exercise the durable journal fallback too: a terminal cursor has already moved on.
            node_id: null,
            request: { type: "action" },
            status: "failed",
            message: "provider unavailable",
            created_at: 0,
            updated_at: 4,
            finished_at: 4,
          },
        ],
        fetch_workflow_journal: [
          {
            version: 1,
            id: "journal-entered",
            workflow_run_id: "run-1",
            sequence: 1,
            continuation_id: "continuation-1",
            entry: { type: "node_entered", continuation_id: "continuation-1", node_id: "tickets" },
            created_at: 0,
          },
          {
            version: 1,
            id: "journal-requested",
            workflow_run_id: "run-1",
            sequence: 2,
            continuation_id: "continuation-1",
            entry: { type: "effect_requested", effect_id: "effect-1", instruction_pointer: 7 },
            created_at: 0,
          },
          {
            version: 1,
            id: "journal-retry-1",
            workflow_run_id: "run-1",
            sequence: 3,
            continuation_id: "continuation-1",
            entry: {
              type: "effect_retry_scheduled",
              effect_id: "effect-1",
              attempt: 1,
              available_at: 1,
            },
            created_at: 1,
          },
          {
            version: 1,
            id: "journal-retry-2",
            workflow_run_id: "run-1",
            sequence: 4,
            continuation_id: "continuation-1",
            entry: {
              type: "effect_retry_scheduled",
              effect_id: "effect-1",
              attempt: 2,
              available_at: 2,
            },
            created_at: 2,
          },
          {
            version: 1,
            id: "journal-retry-3",
            workflow_run_id: "run-1",
            sequence: 5,
            continuation_id: "continuation-1",
            entry: {
              type: "effect_retry_scheduled",
              effect_id: "effect-1",
              attempt: 3,
              available_at: 3,
            },
            created_at: 3,
          },
        ],
        fetch_workflow_vm_cursors: [
          {
            continuation_id: "continuation-1",
            instruction_pointer: 99,
            node_id: "end",
            status: "succeeded",
          },
        ],
      };
      return Promise.resolve(responses[name]);
    });

    const detail = await fetchWorkflowRun("run-1");
    const tickets = detail.nodes.filter((node) => node.node_id === "tickets");

    expect(tickets).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ id: "journal-retry-1", status: "retrying", attempt: 1 }),
        expect.objectContaining({ id: "journal-retry-2", status: "retrying", attempt: 2 }),
        expect.objectContaining({ id: "journal-retry-3", status: "retrying", attempt: 3 }),
        expect.objectContaining({ id: "effect-1", status: "failed", attempt: 3 }),
      ]),
    );
  });
});

describe("command center permissions API in web mode", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.stubGlobal("window", {});
    setCommandRuntime({
      isTauri: () => false,
      invoke: invokeViaHttp,
      wsBaseUrl,
      apiBaseUrl,
    });
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: true,
        status: 200,
        json: vi.fn().mockResolvedValue({}),
      }),
    );
  });

  it("maps user creation to the users endpoint", async () => {
    await createUser({
      username: "ada",
      password: "secret",
      email: "ada@example.com",
      platform_role: "admin",
    });

    expect(fetch).toHaveBeenCalledWith(
      "/api/users",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({
          username: "ada",
          password: "secret",
          email: "ada@example.com",
          platform_role: "admin",
        }),
      }),
    );
  });

  it("clears persisted console state without deleting the session", async () => {
    await clearConsoleSession("00000000-0000-0000-0000-000000000001");

    expect(fetch).toHaveBeenCalledWith(
      "/api/console/sessions/00000000-0000-0000-0000-000000000001/clear",
      expect.objectContaining({ method: "POST" }),
    );
  });

  it("uploads a compiled pack archive with its overwrite choice", async () => {
    const bytes = new Uint8Array([0x50, 0x4b, 0x03, 0x04]).buffer;

    await importPackArchive(bytes, true);

    expect(fetch).toHaveBeenCalledWith(
      "/api/packs/import?overwrite=true",
      expect.objectContaining({
        method: "POST",
        body: bytes,
        headers: expect.objectContaining({ "content-type": "application/zip" }),
      }),
    );
  });

  it("wraps a workflow bundle in a compiled pack zip", async () => {
    vi.mocked(fetch)
      .mockResolvedValueOnce({
        ok: true,
        status: 200,
        json: vi.fn().mockResolvedValue({
          workflows: { workflows: [{ id: "workflow-1" }], triggers: [] },
          secrets: { secrets: [] },
          pipelines: [],
        }),
      } as unknown as Response)
      .mockResolvedValueOnce({
        ok: true,
        status: 200,
        json: vi.fn().mockResolvedValue({ workflows: [{ id: "workflow-1" }], triggers: [] }),
      } as unknown as Response);

    await saveWorkflowBundle({ workflows: [], triggers: [] });

    expect(fetch).toHaveBeenNthCalledWith(
      1,
      "/api/packs/import?overwrite=true",
      expect.objectContaining({
        method: "POST",
        body: expect.any(Blob),
        headers: expect.objectContaining({ "content-type": "application/zip" }),
      }),
    );
    expect(fetch).toHaveBeenNthCalledWith(
      2,
      "/api/workflows/workflow-1/export",
      expect.objectContaining({ headers: {} }),
    );
  });

  it("maps team rename and membership endpoints", async () => {
    await updateTeam("00000000-0000-0000-0000-000000000001", "platform");
    await addTeamMember(
      "00000000-0000-0000-0000-000000000001",
      "00000000-0000-0000-0000-000000000002",
      "member",
    );
    await listTeamMembers("00000000-0000-0000-0000-000000000001");

    expect(fetch).toHaveBeenNthCalledWith(
      1,
      "/api/teams/00000000-0000-0000-0000-000000000001",
      expect.objectContaining({
        method: "PATCH",
        body: JSON.stringify({ name: "platform" }),
      }),
    );
    expect(fetch).toHaveBeenNthCalledWith(
      2,
      "/api/teams/00000000-0000-0000-0000-000000000001/members",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({ user_id: "00000000-0000-0000-0000-000000000002", role: "member" }),
      }),
    );
    expect(fetch).toHaveBeenNthCalledWith(
      3,
      "/api/teams/00000000-0000-0000-0000-000000000001/members",
      expect.objectContaining({ method: "GET" }),
    );
  });

  it("maps api key lifecycle endpoints", async () => {
    await createApiKey({
      name: "deploy",
      principal_kind: "user",
      principal_id: "00000000-0000-0000-0000-000000000002",
      action_ceiling: [],
      expires_at: null,
    });
    await updateApiKey("00000000-0000-0000-0000-000000000003", {
      name: "deploy renamed",
      expires_at: null,
      disabled: false,
    });
    await rotateApiKey("00000000-0000-0000-0000-000000000003");

    expect(fetch).toHaveBeenNthCalledWith(
      1,
      "/api/api_keys",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({
          name: "deploy",
          principal_kind: "user",
          principal_id: "00000000-0000-0000-0000-000000000002",
          action_ceiling: [],
          expires_at: null,
        }),
      }),
    );
    expect(fetch).toHaveBeenNthCalledWith(
      2,
      "/api/api_keys/00000000-0000-0000-0000-000000000003",
      expect.objectContaining({
        method: "PATCH",
        body: JSON.stringify({
          name: "deploy renamed",
          expires_at: null,
          disabled: false,
        }),
      }),
    );
    expect(fetch).toHaveBeenNthCalledWith(
      3,
      "/api/api_keys/00000000-0000-0000-0000-000000000003/rotate",
      expect.objectContaining({ method: "POST" }),
    );
  });
});

describe("run side-channel endpoints in web mode", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.stubGlobal("window", {});
    setCommandRuntime({
      isTauri: () => false,
      invoke: invokeViaHttp,
      wsBaseUrl,
      apiBaseUrl,
    });
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: true,
        status: 200,
        json: vi.fn().mockResolvedValue({}),
      }),
    );
  });

  it("posts an interrupt request with its optional fields", async () => {
    await requestRunInterrupt(
      "00000000-0000-0000-0000-000000000080",
      "external",
      { why: "manual" },
      "00000000-0000-0000-0000-000000000081",
    );

    expect(fetch).toHaveBeenCalledWith(
      "/api/workflow_runs/00000000-0000-0000-0000-000000000080/interrupts",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({
          source: "external",
          payload: { why: "manual" },
          continuation_id: "00000000-0000-0000-0000-000000000081",
        }),
      }),
    );
  });

  /** signals had a tauri command but no http descriptor, so the web build threw on delivery. */
  it("posts a signal delivery", async () => {
    await deliverSignal("00000000-0000-0000-0000-000000000080", "approved", { by: "ada" });

    expect(fetch).toHaveBeenCalledWith(
      "/api/workflow_runs/00000000-0000-0000-0000-000000000080/signals",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({ name: "approved", payload: { by: "ada" } }),
      }),
    );
  });

  it("maps the orchestration correlation alias lifecycle", async () => {
    const orchestrationId = "00000000-0000-0000-0000-000000000090";
    const aliasId = "00000000-0000-0000-0000-000000000091";

    await fetchOrchestrationAliases(orchestrationId);
    await addOrchestrationAlias(orchestrationId, "github", "pull-requests", "octo/repo#42");
    await deleteOrchestrationAlias(orchestrationId, aliasId);

    expect(fetch).toHaveBeenNthCalledWith(
      1,
      `/api/orchestrations/${orchestrationId}/aliases`,
      expect.objectContaining({ method: "GET" }),
    );
    expect(fetch).toHaveBeenNthCalledWith(
      2,
      `/api/orchestrations/${orchestrationId}/aliases`,
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({
          source: "github",
          scope: "pull-requests",
          correlation_key: "octo/repo#42",
        }),
      }),
    );
    expect(fetch).toHaveBeenNthCalledWith(
      3,
      `/api/orchestrations/${orchestrationId}/aliases/${aliasId}`,
      expect.objectContaining({ method: "DELETE" }),
    );
  });
});
