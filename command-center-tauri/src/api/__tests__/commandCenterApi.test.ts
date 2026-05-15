import { beforeEach, describe, expect, it, vi } from "vitest";
import { fetchWorkflowNodeRunArtifacts, fetchWorkflowNodeRunChunks } from "../commandCenterApi";
import { invoke } from "@tauri-apps/api/core";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn()
}));

describe("command center workflow node run API", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockResolvedValue([]);
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
  });

  it("requests workflow node run chunks by node run id", async () => {
    await fetchWorkflowNodeRunChunks(42);

    expect(invoke).toHaveBeenCalledWith("fetch_workflow_node_run_chunks", { nodeRunId: 42 });
  });

  it("requests workflow node run artifacts by node run id", async () => {
    await fetchWorkflowNodeRunArtifacts(42);

    expect(invoke).toHaveBeenCalledWith("fetch_workflow_node_run_artifacts", { nodeRunId: 42 });
  });
});
