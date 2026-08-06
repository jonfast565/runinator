import { beforeEach, describe, expect, it, vi } from "vitest";
import { createWorkflowRevisionsService } from "../workflow-revisions";
import { revisionAuthorLabel } from "../../domain/models";
import type { WorkflowRevision } from "../../domain/models";
import type { AppService, RunOperationOptions } from "../app";

vi.mock("../../api/commandCenterApi", () => ({
  fetchWorkflowRevisions: vi.fn(),
  fetchWorkflowRevision: vi.fn(),
  restoreWorkflowRevision: vi.fn(),
}));

import {
  fetchWorkflowRevision,
  fetchWorkflowRevisions,
  restoreWorkflowRevision,
} from "../../api/commandCenterApi";

function revision(overrides: Partial<WorkflowRevision> = {}): WorkflowRevision {
  return {
    id: "revision-1",
    workflow_id: "workflow-1",
    revision: 1,
    version: "1.0.0",
    name: "ticket work",
    input_type: {},
    definition: { nodes: [] },
    source: "ui",
    actor_id: null,
    actor_kind: "system",
    note: null,
    created_at: "2026-08-05T00:00:00Z",
    ...overrides,
  };
}

// records the options each call was made with, so the retryable policy is assertable.
const calls: { label: string; options?: RunOperationOptions }[] = [];

const app = {
  runOperation: <T>(label: string, run: () => Promise<T>, options?: RunOperationOptions) => {
    calls.push({ label, options });
    return run();
  },
  setError: () => undefined,
} as unknown as AppService;

describe("workflow revisions service", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    calls.length = 0;
  });

  it("lists revisions for a workflow", async () => {
    vi.mocked(fetchWorkflowRevisions).mockResolvedValue([revision({ revision: 2 }), revision()]);
    const service = createWorkflowRevisionsService(app);

    const revisions = await service.list("workflow-1", 10);

    expect(fetchWorkflowRevisions).toHaveBeenCalledWith("workflow-1", 10);
    expect(revisions.map((r) => r.revision)).toEqual([2, 1]);
  });

  it("fetches a single revision with its definition", async () => {
    vi.mocked(fetchWorkflowRevision).mockResolvedValue(
      revision({ definition: { nodes: [{ id: "start" }] } }),
    );
    const service = createWorkflowRevisionsService(app);

    const found = await service.get("workflow-1", 1);

    expect(fetchWorkflowRevision).toHaveBeenCalledWith("workflow-1", 1);
    expect(found.definition).toEqual({ nodes: [{ id: "start" }] });
  });

  it("restores a revision", async () => {
    vi.mocked(restoreWorkflowRevision).mockResolvedValue({
      id: "workflow-1",
      name: "ticket work",
      version: "1.0.0",
      enabled: true,
      input_type: {},
      definition: { nodes: [] },
    });
    const service = createWorkflowRevisionsService(app);

    const restored = await service.restore("workflow-1", 1);

    expect(restoreWorkflowRevision).toHaveBeenCalledWith("workflow-1", 1);
    expect(restored.id).toBe("workflow-1");
  });

  it("offers retry on reads but not on a restore", async () => {
    vi.mocked(fetchWorkflowRevisions).mockResolvedValue([]);
    vi.mocked(restoreWorkflowRevision).mockResolvedValue({
      id: "workflow-1",
      name: "ticket work",
      version: "1.0.0",
      enabled: true,
      input_type: {},
      definition: {},
    });
    const service = createWorkflowRevisionsService(app);

    await service.list("workflow-1");
    await service.restore("workflow-1", 1);

    // a restore creates a revision, so retrying one that failed after it landed would stack a
    // second identical rollback onto the history.
    expect(calls[0].options?.retryable).toBe(true);
    expect(calls[1].options?.retryable).toBeUndefined();
  });

  it("labels an unattributed revision by its kind rather than a blank", () => {
    expect(revisionAuthorLabel(revision())).toBe("system");
    expect(revisionAuthorLabel(revision({ actor_id: "user-7", actor_kind: "user" }))).toBe(
      "user · user-7",
    );
  });
});
