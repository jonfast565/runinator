import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { useWorkflowsStore } from "../workflows";
import type { WorkflowDefinition, WorkflowRunDetail } from "../../../../core/domain/models";

vi.mock("../../../../core/api/commandCenterApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../../core/api/commandCenterApi")>()),
  fetchWorkflowRun: vi.fn(),
  requestRunInterrupt: vi.fn(),
  decompileToRexRap: vi.fn(),
}));

import {
  decompileToRexRap,
  fetchWorkflowRun,
  requestRunInterrupt,
} from "../../../../core/api/commandCenterApi";
import { setWorkflowCatalogs } from "../../../../core/workflow/catalog-registry";
import { testNodeKindCatalog } from "../../../../core/workflow/__tests__/catalog-fixtures";

const RUN_ID = "00000000-0000-0000-0000-000000000080";

/** a run whose workflow snapshot declares a `wake` handler. */
function detail(status = "running", withHandler = true): WorkflowRunDetail {
  const snapshot = {
    name: "Interruptible",
    definition: {
      start: "start",
      nodes: [
        { id: "start", kind: "start", transitions: { next: { $node: "end" } } },
        { id: "end", kind: "end" },
        { id: "on_wake", kind: "audit", transitions: { next: { $node: "resume_wake" } } },
        { id: "resume_wake", kind: "resume" },
      ],
      metadata: withHandler ? { interrupts: [{ on: "wake", handler: "on_wake" }] } : {},
    },
  } as unknown as WorkflowDefinition;

  return {
    run: { id: RUN_ID, workflow_id: "wf-1", status, workflow_snapshot: snapshot },
    nodes: [],
  } as unknown as WorkflowRunDetail;
}

async function selectRun(overrides?: WorkflowRunDetail) {
  const workflows = useWorkflowsStore();
  vi.mocked(fetchWorkflowRun).mockResolvedValue(overrides ?? detail());
  await workflows.fetchWorkflowRunDetail(RUN_ID, true);
  return workflows;
}

beforeEach(() => {
  setActivePinia(createPinia());
  setWorkflowCatalogs({ nodeKinds: testNodeKindCatalog, triggerKinds: [], enums: [] });
  useWorkflowsStore().clearServiceState({ discardDraft: true });
  vi.stubGlobal("window", {
    clearTimeout: () => undefined,
    setTimeout: (fn: () => void) => {
      fn();
      return 0;
    },
  });
  vi.clearAllMocks();
  vi.mocked(decompileToRexRap).mockResolvedValue("workflow stub { start -> end }");
  vi.mocked(requestRunInterrupt).mockResolvedValue({ success: true, message: "recorded" });
});

afterEach(() => {
  setWorkflowCatalogs({ nodeKinds: [], triggerKinds: [], enums: [] });
});

describe("requesting an interrupt", () => {
  it("posts the source, payload, and continuation", async () => {
    const workflows = await selectRun();

    await expect(
      workflows.requestSelectedRunInterrupt("wake", { why: "manual" }, "continuation-1"),
    ).resolves.toBe(true);

    expect(requestRunInterrupt).toHaveBeenCalledWith(
      RUN_ID,
      "wake",
      { why: "manual" },
      "continuation-1",
    );
  });

  it("offers only enabled, declared handlers that can be explicitly requested", async () => {
    const workflows = await selectRun();

    expect(workflows.requestableInterruptSources).toEqual(["wake"]);
  });

  it("does not offer disabled, undeclared, or scheduler-owned timer handlers", async () => {
    const run = detail();
    const snapshot = run.run.workflow_snapshot as unknown as WorkflowDefinition;
    snapshot.definition.metadata = {
      interrupts: [
        { on: "external", handler: "on_external", enabled: false },
        { on: "timer", handler: "on_timer", interval_seconds: 60 },
      ],
    };
    const workflows = await selectRun(run);

    expect(workflows.requestableInterruptSources).toEqual([]);
    expect(workflows.canRequestRunInterrupt).toBe(false);
  });

  /** without a handler the reducer records the request and then drops it, so do not offer it. */
  it("is refused when the workflow declares no handlers", async () => {
    const workflows = await selectRun(detail("running", false));

    expect(workflows.canRequestRunInterrupt).toBe(false);

    await workflows.requestSelectedRunInterrupt("external");

    expect(requestRunInterrupt).not.toHaveBeenCalled();
  });

  it("is refused on a terminal run", async () => {
    const workflows = await selectRun(detail("succeeded"));

    expect(workflows.canRequestRunInterrupt).toBe(false);

    await workflows.requestSelectedRunInterrupt("wake");

    expect(requestRunInterrupt).not.toHaveBeenCalled();
  });

  it("refuses a source without an enabled handler", async () => {
    const workflows = await selectRun();

    await expect(workflows.requestSelectedRunInterrupt("external")).resolves.toBe(false);

    expect(requestRunInterrupt).not.toHaveBeenCalled();
  });

  it("reports a failed request rather than claiming it was recorded", async () => {
    const workflows = await selectRun();
    vi.mocked(requestRunInterrupt).mockResolvedValue({ success: false, message: "run is done" });

    await expect(workflows.requestSelectedRunInterrupt("wake")).resolves.toBe(false);

    // the detail refresh only happens on success; a failure surfaces as an error toast instead.
    expect(vi.mocked(fetchWorkflowRun)).toHaveBeenCalledTimes(1);
  });
});
