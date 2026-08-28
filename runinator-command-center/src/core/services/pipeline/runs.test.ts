import { beforeEach, describe, expect, it, vi } from "vitest";
import { deletePipelineRuns } from "./runs";

vi.mock("../../api/commandCenterApi", () => ({
  deletePipelineRun: vi.fn(),
}));

import { deletePipelineRun } from "../../api/commandCenterApi";

describe("pipeline run history deletion", () => {
  beforeEach(() => {
    vi.resetAllMocks();
  });

  it("deletes every requested run while retaining failures for a targeted retry", async () => {
    vi.mocked(deletePipelineRun)
      .mockResolvedValueOnce({ success: true, message: "deleted" })
      .mockResolvedValueOnce({ success: false, message: "run is protected" });

    const result = await deletePipelineRuns(["pipeline-run-1", "pipeline-run-2"]);

    expect(deletePipelineRun).toHaveBeenNthCalledWith(1, "pipeline-run-1");
    expect(deletePipelineRun).toHaveBeenNthCalledWith(2, "pipeline-run-2");
    expect(result.succeeded).toEqual(["pipeline-run-1"]);
    expect(result.failed).toHaveLength(1);
    expect(result.failed[0]?.item).toBe("pipeline-run-2");
    expect(result.failed[0]?.message).toContain("protected");
  });
});
