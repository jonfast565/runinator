import { describe, expect, it } from "vitest";

import { buildSimPreview, simTone } from "../run-simulation";
import type { SimulationRun } from "../../domain/models";

describe("simTone", () => {
  it("classifies known statuses and defaults unknown ones to muted", () => {
    expect(simTone("succeeded")).toBe("ok");
    expect(simTone("Failed")).toBe("bad");
    expect(simTone("waiting")).toBe("warn");
    expect(simTone("something-else")).toBe("muted");
  });
});

describe("buildSimPreview", () => {
  it("maps steps to rows and carries branch targets and notes", () => {
    const run: SimulationRun = {
      status: "succeeded",
      output: { ok: true },
      steps: [
        { node_id: "start", kind: "start", status: "succeeded", next: "charge" },
        {
          node_id: "charge",
          kind: "action",
          status: "succeeded",
          next: "done",
          output: { charged: 10 },
          note: "on_success",
        },
        { node_id: "done", kind: "end", status: "succeeded" },
      ],
    };

    const preview = buildSimPreview(run);

    expect(preview.rows).toHaveLength(3);
    expect(preview.rows[1]).toMatchObject({
      nodeId: "charge",
      branch: "done",
      note: "on_success",
      tone: "ok",
    });
    expect(preview.rows[2].branch).toBeNull();
    expect(preview.reachedCount).toBe(3);
    expect(preview.error).toBeNull();
    expect(preview.outputJson).toContain("\"ok\": true");
  });

  it("marks a stuck walk as bad and surfaces the error, and reports null output", () => {
    const run: SimulationRun = {
      status: "failed",
      output: null,
      error: "node 'fan' of kind parallel is not supported by the dry-run simulator",
      steps: [{ node_id: "start", kind: "start", status: "succeeded", next: "fan" }],
    };

    const preview = buildSimPreview(run);

    expect(preview.tone).toBe("bad");
    expect(preview.error).toContain("not supported");
    expect(preview.outputJson).toBeNull();
  });
});
