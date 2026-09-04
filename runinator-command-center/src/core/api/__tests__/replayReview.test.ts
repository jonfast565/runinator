import { afterEach, describe, expect, it, vi } from "vitest";
import { replayWorkflowRun } from "../commandCenterApi";
import { setReplayPlanReviewer } from "../replayReview";
import { setCommandRuntime } from "../runtime";
import type { ReplayPlan } from "../../domain/models/workflow/replay";

const plan: ReplayPlan = {
  source_run_id: "run",
  from_step_id: null,
  workflow_snapshot: null,
  seeded_receipts: [],
  actions: [],
  reasons: ["duplicate effects possible"],
  verdict: "review",
  plan_fingerprint: "sha256:reviewed",
};

afterEach(() => {
  setReplayPlanReviewer(undefined);
});

function setup(value = plan) {
  const invoke = vi
    .fn()
    .mockImplementation((name: string) =>
      Promise.resolve(name === "fetch_replay_plan" ? value : { id: "created" }),
    );
  setCommandRuntime({ invoke, isTauri: () => false, wsBaseUrl: () => "", apiBaseUrl: () => "" });

  return invoke;
}

describe("all replay entrypoints require a reviewed plan", () => {
  it("fails closed when review UI is unavailable or canceled", async () => {
    const invoke = setup();
    await expect(replayWorkflowRun("run")).rejects.toThrow("canceled");
    expect(invoke).toHaveBeenCalledTimes(1);
    setReplayPlanReviewer(() => Promise.resolve(false));
    await expect(replayWorkflowRun("run")).rejects.toThrow("canceled");
    expect(invoke).toHaveBeenCalledTimes(2);
  });
  it("binds mutation to the displayed fingerprint and preserves managed override", async () => {
    const invoke = setup();
    const reviewer = vi.fn().mockResolvedValue(true);
    setReplayPlanReviewer(reviewer);
    await replayWorkflowRun("run", {
      fromStepId: "b",
      override: { reason: "emergency", idempotencyKey: "once" },
    });
    expect(reviewer).toHaveBeenCalledWith(plan);
    expect(invoke).toHaveBeenLastCalledWith("replay_workflow_run", {
      workflowRunId: "run",
      fromStepId: "b",
      planFingerprint: plan.plan_fingerprint,
      acknowledgeReview: true,
      overrideReason: "emergency",
      idempotencyKey: "once",
    });
  });
  it("cannot authorize a blocked plan even if the reviewer returns true", async () => {
    const invoke = setup({ ...plan, verdict: "blocked" });
    setReplayPlanReviewer(() => Promise.resolve(true));
    await expect(replayWorkflowRun("run")).rejects.toThrow("canceled");
    expect(invoke).toHaveBeenCalledTimes(1);
  });
});
