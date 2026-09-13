import { describe, expect, it } from "vitest";
import type { OrchestrationPolicy } from "../../domain/models";
import {
  derivePhasePolicyProfiles,
  expandPhasePolicyProfiles,
  loadPhasePolicyProfiles,
  phasePolicyProfilesMetadata,
} from "../orchestration-phase-profiles";

const members = ["plan", "build", "review"];

function policyPhases(): OrchestrationPolicy["phases"] {
  return {
    plan: {
      result: { resources_patch: "/resources_patch", next_member: "/next_member" },
      workspace: {
        scope: "source",
        requirements: { os: "linux", capability: "git" },
        lease_seconds: 600,
        reuse: true,
        recovery: "wait",
      },
    },
    build: {
      result: { resources_patch: "/resources_patch", next_member: "/next_member" },
      workspace: {
        scope: "source",
        requirements: { capability: "git", os: "linux" },
        lease_seconds: 600,
        reuse: true,
        recovery: "wait",
      },
    },
    review: { result: { evidence: "/evidence" } },
  };
}

describe("orchestration phase profiles", () => {
  it("groups structurally identical legacy mappings and workspaces", () => {
    const profiles = derivePhasePolicyProfiles(members, policyPhases());

    expect(profiles.result_mappings).toHaveLength(2);
    expect(profiles.workspace_policies).toHaveLength(1);
    expect(profiles.assignments.plan).toEqual(profiles.assignments.build);
    expect(profiles.assignments.review).not.toEqual(profiles.assignments.plan);
  });

  it("round trips every result field through the expanded runtime shape", () => {
    const phases: OrchestrationPolicy["phases"] = {
      plan: {
        result: {
          subject_revision: "/revision",
          resources: "/resources",
          evidence: "/evidence",
          failure_class: "/failure",
          correlations: "/correlations",
          resources_patch: "/patch",
          next_member: "/next",
        },
      },
    };
    const profiles = derivePhasePolicyProfiles(["plan"], phases);

    expect(expandPhasePolicyProfiles(["plan"], profiles)).toEqual(phases);
  });

  it("preserves saved names and unassigned profiles when metadata matches runtime", () => {
    const profiles = derivePhasePolicyProfiles(members, policyPhases());
    profiles.result_mappings[0].name = "Progress results";
    profiles.result_mappings.push({ id: "unused", name: "Future mapping", mapping: {} });
    const authoring = { schema_version: 2, phase_profiles: phasePolicyProfilesMetadata(profiles) };

    const loaded = loadPhasePolicyProfiles(members, policyPhases(), authoring);

    expect(loaded.result_mappings.map((profile) => profile.name)).toContain("Progress results");
    expect(loaded.result_mappings.map((profile) => profile.name)).toContain("Future mapping");
  });

  it("falls back to runtime data when saved assignments drift", () => {
    const profiles = derivePhasePolicyProfiles(members, policyPhases());
    profiles.assignments.plan = {};

    const loaded = loadPhasePolicyProfiles(members, policyPhases(), {
      schema_version: 2,
      phase_profiles: phasePolicyProfilesMetadata(profiles),
    });

    expect(loaded.assignments.plan.result_mapping_id).toBeDefined();
    expect(loaded.result_mappings[0].name).toBe("Mapping 1");
  });

  it("drops removed members and leaves new members unassigned", () => {
    const profiles = derivePhasePolicyProfiles(members, policyPhases());
    const authoring = { schema_version: 2, phase_profiles: phasePolicyProfilesMetadata(profiles) };
    const currentMembers = ["plan", "build", "review", "publish"];
    const currentPhases = { ...policyPhases(), publish: { result: {} } };

    const loaded = loadPhasePolicyProfiles(currentMembers, currentPhases, authoring);

    expect(loaded.assignments.publish).toEqual({});
    expect(Object.keys(loaded.assignments)).toEqual(currentMembers);
  });
});
