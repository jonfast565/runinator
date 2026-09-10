import { describe, expect, it, vi } from "vitest";
import { createWorkflowsCommands } from "../commands/workflows";
import type { ConsoleWorkflowsApi } from "../../api/ports/console-workflows";
import type { ConsoleCommandContext } from "../types";

function workflowApi(id: string) {
  return {
    fetchWorkflows: vi
      .fn<ConsoleWorkflowsApi["fetchWorkflows"]>()
      .mockResolvedValue([
        { id, name: "daily", version: "1.0.0", enabled: true, input_type: {}, definition: {} },
      ]),
    fetchPipelines: vi.fn<ConsoleWorkflowsApi["fetchPipelines"]>(),
    createWorkflowRun: vi
      .fn<ConsoleWorkflowsApi["createWorkflowRun"]>()
      .mockResolvedValue({ id: "run" }),
    duplicateWorkflow: vi.fn<ConsoleWorkflowsApi["duplicateWorkflow"]>(),
    exportWorkflowBundle: vi.fn<ConsoleWorkflowsApi["exportWorkflowBundle"]>(),
    fetchWorkflowRevision: vi.fn<ConsoleWorkflowsApi["fetchWorkflowRevision"]>(),
    fetchWorkflowRevisions: vi.fn<ConsoleWorkflowsApi["fetchWorkflowRevisions"]>(),
    restoreWorkflowRevision: vi.fn<ConsoleWorkflowsApi["restoreWorkflowRevision"]>(),
  } satisfies ConsoleWorkflowsApi;
}

const context: ConsoleCommandContext = {
  args: ["daily"],
  flags: {},
  json: false,
  signal: new AbortController().signal,
  print: vi.fn(),
  terminal: { clear: vi.fn() },
  session: {
    current: () => null,
    list: () => [],
    cells: () => [],
    refresh: vi.fn(),
    open: vi.fn(),
    create: vi.fn(),
    remove: vi.fn(),
    cancelCell: vi.fn(),
    replayCell: vi.fn(),
  },
};

describe("injected console API", () => {
  it("uses the same instance for name lookup and execution", async () => {
    const first = workflowApi("first");
    const second = workflowApi("second");

    for (const api of [first, second]) {
      const { workflowCommands } = createWorkflowsCommands(api);
      await workflowCommands
        .find((command) => command.path.join(" ") === "workflows run")!
        .run(context);
    }

    expect(first.createWorkflowRun).toHaveBeenCalledWith("first", { debug: false, parameters: {} });
    expect(second.createWorkflowRun).toHaveBeenCalledWith("second", {
      debug: false,
      parameters: {},
    });
    expect(first.fetchWorkflows).toHaveBeenCalledOnce();
    expect(second.fetchWorkflows).toHaveBeenCalledOnce();
  });

  it("does not execute when lookup fails", async () => {
    const api = workflowApi("first");
    api.fetchWorkflows.mockRejectedValue(new Error("unavailable"));
    const { workflowCommands } = createWorkflowsCommands(api);
    await expect(
      workflowCommands.find((command) => command.path.join(" ") === "workflows run")!.run(context),
    ).rejects.toThrow("unavailable");
    expect(api.createWorkflowRun).not.toHaveBeenCalled();
  });
});
