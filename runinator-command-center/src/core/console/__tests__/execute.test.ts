// covers dispatch: which command a line selects, what it prints, and how a line that cannot work
// here reports itself.

import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../api/commandCenterApi", () => ({
  fetchWorkflows: vi.fn(),
  fetchWorkflowRuns: vi.fn(),
  fetchSupervisorStatus: vi.fn(),
  fetchApprovals: vi.fn(),
  approveApproval: vi.fn(),
  rejectApproval: vi.fn(),
  fetchProviders: vi.fn(),
  fetchWorkflowEffectOutput: vi.fn(),
  createWorkflowRun: vi.fn(),
  duplicateWorkflow: vi.fn(),
  exportWorkflowBundle: vi.fn(),
  fetchWorkflowRevision: vi.fn(),
  fetchWorkflowRevisions: vi.fn(),
  restoreWorkflowRevision: vi.fn(),
  cancelWorkflowRun: vi.fn(),
  fetchWorkflowRun: vi.fn(),
  fetchWorkflowRunArtifacts: vi.fn(),
  pauseWorkflowRun: vi.fn(),
  renameWorkflowRun: vi.fn(),
  replayWorkflowRun: vi.fn(),
  resumeWorkflowRun: vi.fn(),
  backfillWorkflowTrigger: vi.fn(),
  createFreezeWindow: vi.fn(),
  createTriggerRun: vi.fn(),
  deleteFreezeWindow: vi.fn(),
  fetchDueTriggers: vi.fn(),
  fetchFreezeWindows: vi.fn(),
  fetchWorkflowTriggers: vi.fn(),
  deleteFunctionAlias: vi.fn(),
  deleteFunctionPackage: vi.fn(),
  fetchFunctionCatalog: vi.fn(),
  fetchFunctionPackage: vi.fn(),
  fetchFunctionPackages: vi.fn(),
  invokeFunction: vi.fn(),
  setFunctionAlias: vi.fn(),
  deleteCredential: vi.fn(),
  fetchCredential: vi.fn(),
  fetchCredentials: vi.fn(),
  saveCredential: vi.fn(),
  analyzeRexRap: vi.fn(),
  compileRexRap: vi.fn(),
  decompileToRexRap: vi.fn(),
  formatRexRap: vi.fn(),
  createAgentDirective: vi.fn(),
  createAgentEnrollmentToken: vi.fn(),
  createOrg: vi.fn(),
  fetchNodeBackends: vi.fn(),
  fetchNodes: vi.fn(),
  fetchOrgNodes: vi.fn(),
  fetchOrgUsage: vi.fn(),
  listAgentDirectives: vi.fn(),
  listAgentEnrollmentTokens: vi.fn(),
  listMyOrgs: vi.fn(),
  revokeAgentEnrollmentToken: vi.fn(),
  scaleNodes: vi.fn(),
  scaleOrgNodes: vi.fn(),
  stopNode: vi.fn(),
  createPipelineRun: vi.fn(),
  fetchPipelines: vi.fn(),
  fetchReplicas: vi.fn(),
  fetchReplicaProviders: vi.fn(),
  fetchReplicaSamples: vi.fn(),
}));

import {
  createWorkflowRun,
  fetchCredentials,
  fetchReplicas,
  fetchWorkflows,
  invokeFunction,
} from "../../api/commandCenterApi";
import { executeCommand } from "../execute";
import type { ConsoleOutput, ConsoleSessionPort } from "../types";
import type { WorkflowDefinition } from "../../domain/models";

function workflow(overrides: Partial<WorkflowDefinition> = {}): WorkflowDefinition {
  return {
    id: "11111111-1111-4111-8111-111111111111",
    name: "daily",
    version: "1.0.0",
    enabled: true,
    input_type: {},
    definition: {},
    ...overrides,
  };
}

const session: ConsoleSessionPort = {
  current: () => null,
  list: () => [],
  refresh: () => Promise.resolve(),
  open: () => Promise.resolve(),
  create: () => Promise.reject(new Error("not used")),
  remove: () => Promise.resolve(),
  cells: () => [],
  cancelCell: () => Promise.resolve(),
  replayCell: () => Promise.reject(new Error("not used")),
};

async function run(line: string): Promise<ConsoleOutput[]> {
  const outputs: ConsoleOutput[] = [];
  await executeCommand(line, {
    session,
    terminal: { clear: () => undefined },
    signal: new AbortController().signal,
    print: (output) => outputs.push(output),
  });
  return outputs;
}

beforeEach(() => {
  vi.clearAllMocks();
});

describe("executeCommand", () => {
  it("prints a table for workflows list", async () => {
    vi.mocked(fetchWorkflows).mockResolvedValue([workflow()]);

    const [output] = await run("workflows list");

    expect(output).toMatchObject({ kind: "table" });
    expect(output.kind === "table" && output.rows[0]).toContain("daily");
  });

  it("returns the raw payload for --json", async () => {
    vi.mocked(fetchWorkflows).mockResolvedValue([workflow()]);

    const [output] = await run("workflows list --json");

    expect(output.kind).toBe("json");
  });

  it("resolves a workflow by name before starting a run", async () => {
    vi.mocked(fetchWorkflows).mockResolvedValue([workflow()]);
    vi.mocked(createWorkflowRun).mockResolvedValue({ id: "run-1" });

    await run("workflows run daily --param count=2 --debug");

    expect(createWorkflowRun).toHaveBeenCalledWith("11111111-1111-4111-8111-111111111111", {
      debug: true,
      parameters: { count: 2 },
    });
  });

  it("takes the longest matching command path", async () => {
    vi.mocked(fetchCredentials).mockResolvedValue([{ scope: "aws", name: "key", kind: "secret" }]);

    const [output] = await run("settings list --kind secret");

    expect(output.kind === "table" && output.rows[0]).toEqual(["secret", "aws", "key"]);
  });

  it("lists the active session function library", async () => {
    const outputs: ConsoleOutput[] = [];
    await executeCommand("functions", {
      session: {
        ...session,
        current: () => ({
          id: "session-1",
          name: "scratch",
          created_at: "2026-08-25T00:00:00Z",
          updated_at: "2026-08-25T00:00:00Z",
          functions: [
            {
              id: "function-1",
              session_id: "session-1",
              cell_id: "cell-1",
              name: "double",
              is_task: false,
              source: "fn double(x: integer) = x * 2",
              created_at: "2026-08-25T00:00:00Z",
              updated_at: "2026-08-25T00:00:00Z",
            },
          ],
        }),
      },
      terminal: { clear: () => undefined },
      signal: new AbortController().signal,
      print: (output) => outputs.push(output),
    });

    expect(outputs[0]).toMatchObject({
      kind: "table",
      columns: ["name", "kind", "cell", "source"],
    });
    expect(outputs[0].kind === "table" && outputs[0].rows[0]).toContain("double");
  });

  it("splits package.export on the last dot and passes the selector", async () => {
    vi.mocked(invokeFunction).mockResolvedValue({ ok: true });

    await run(`invoke images.thumbs.resize --alias production --input '{"width":320}'`);

    expect(invokeFunction).toHaveBeenCalledWith(
      "images.thumbs",
      "resize",
      { width: 320 },
      { alias: "production", version: undefined },
    );
  });

  it("reports an unknown command", async () => {
    await expect(run("nonsense")).rejects.toThrow(/unknown console command/);
  });

  it("suggests the nearest verb for a typo", async () => {
    await expect(run("wrkflows list")).rejects.toThrow(/did you mean ':workflows'/);
  });

  it("rejects a flag the command does not take", async () => {
    // a mistyped filter used to be ignored, which made `--stauts failed` read as 'every run'.
    await expect(run("runs list --stauts failed")).rejects.toThrow(/unexpected argument '--stauts'/);
  });

  it("rejects a value outside a flag's closed set", async () => {
    await expect(run("settings list --kind sekret")).rejects.toThrow(/invalid value 'sekret'/);
  });

  it("lists the replicas a filter selects", async () => {
    vi.mocked(fetchReplicas).mockResolvedValue({
      counts: { workers: 1, wakers: 0, webservices: 0, background: 0 },
      replicas: [
        {
          replica_id: "r-1",
          replica_type: "worker",
          instance_id: "worker-a",
          runtime_id: "run-1",
          status: "live",
          attributes: {},
          first_seen_at: "2026-08-01T00:00:00Z",
          last_heartbeat_at: "2026-08-01T00:05:00Z",
          last_seen_at: "2026-08-01T00:05:00Z",
        },
        {
          replica_id: "r-2",
          replica_type: "waker",
          instance_id: "waker-a",
          runtime_id: "run-2",
          status: "offline",
          attributes: {},
          first_seen_at: "2026-08-01T00:00:00Z",
          last_heartbeat_at: "2026-08-01T00:01:00Z",
          last_seen_at: "2026-08-01T00:01:00Z",
        },
      ],
    });

    const [output] = await run("replicas list --live");

    expect(output.kind === "table" && output.rows).toEqual([
      ["r-1", "worker", "live", "worker-a", "-", "2026-08-01 00:05"],
    ]);
  });

  it("says where a local-only command has to run instead", async () => {
    await expect(run("workflows apply ./packs")).rejects.toThrow(/runinatorctl/);
  });

  it("lists the commands for help", async () => {
    const outputs = await run("help");
    const listed = outputs.find((output) => output.kind === "table");

    expect(listed?.kind === "table" && listed.rows.map(([command]) => command)).toEqual(
      expect.arrayContaining([":workflows list", ":runs watch", ":agents drain"]),
    );
  });
});
