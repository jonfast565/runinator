import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../api/commandCenterApi", () => ({
  fetchPipelines: vi.fn(),
  fetchWorkflows: vi.fn(),
}));

import { fetchPipelines } from "../../api/commandCenterApi";
import { defaultPipelineDefaults, pipelinePath, type Pipeline } from "../../domain/models";
import { resolvePipeline } from "../lookup";

const pipeline: Pipeline = {
  id: "11111111-1111-4111-8111-111111111111",
  name: "Release train",
  description: null,
  key: "release_train",
  namespace: "acme.delivery",
  graph: { version: 1, members: [], links: [], joins: {} },
  concurrency: { max_concurrent_runs: 0, on_conflict: "allow" },
  defaults: defaultPipelineDefaults(),
  metadata: {},
};

beforeEach(() => {
  vi.clearAllMocks();
});

describe("resolvePipeline", () => {
  it("accepts a UUID or canonical namespace.key path", async () => {
    vi.mocked(fetchPipelines).mockResolvedValue([pipeline]);

    await expect(resolvePipeline(pipeline.id!)).resolves.toEqual(pipeline);
    await expect(resolvePipeline(pipelinePath(pipeline))).resolves.toEqual(pipeline);
  });

  it("does not resolve display names or bare keys", async () => {
    vi.mocked(fetchPipelines).mockResolvedValue([pipeline]);

    await expect(resolvePipeline(pipeline.name)).rejects.toThrow(/not found/);
    await expect(resolvePipeline(pipeline.key!)).rejects.toThrow(/not found/);
  });
});
