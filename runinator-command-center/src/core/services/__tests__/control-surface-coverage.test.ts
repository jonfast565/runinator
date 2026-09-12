import { describe, expect, it } from "vitest";
import { tabs } from "../../navigation/nav-config";
import { controlSurfaceCoverage, coverageRequirements } from "../control-surface-coverage";

describe("control surface coverage", () => {
  it("answers every accessibility requirement for every capability", () => {
    const ids = new Set<string>();

    for (const capability of controlSurfaceCoverage.capabilities) {
      expect(ids.has(capability.id), capability.id).toBe(false);
      ids.add(capability.id);
      expect(capability.summary.trim(), capability.id).not.toBe("");
      expect(capability.tabs.length, capability.id).toBeGreaterThan(0);

      for (const tab of capability.tabs) {
        expect(tabs, `${capability.id}: ${tab}`).toContain(tab);
      }

      for (const requirement of coverageRequirements) {
        const answer = capability.coverage[requirement];
        expect(["surface", "intentional", "gap"], `${capability.id}: ${requirement}`).toContain(
          answer.outcome,
        );
        expect(answer.detail.trim(), `${capability.id}: ${requirement}`).not.toBe("");
      }
    }
  });

  it("ships Phase 5 without an unrecorded capability gap", () => {
    const gaps = controlSurfaceCoverage.capabilities.flatMap((capability) =>
      coverageRequirements
        .filter((requirement) => capability.coverage[requirement].outcome === "gap")
        .map((requirement) => `${capability.id}:${requirement}`),
    );

    expect(gaps).toEqual([]);
  });
});
