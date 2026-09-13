import { describe, expect, it } from "vitest";
import {
  missionPhaseSource,
  missionRecipeFromPipeline,
  missionRecipePreset,
  switchMissionAgentRuntime,
  validateMissionRecipe,
} from "../mission-recipes";

describe("mission recipe authoring", () => {
  it("treats coding and research as editable preset drafts", () => {
    const coding = missionRecipePreset("coding");
    const research = missionRecipePreset("research_report");

    expect(validateMissionRecipe(coding)).toEqual([]);
    expect(validateMissionRecipe(research)).toEqual([]);
    expect(coding.phases.map((phase) => phase.id)).toEqual([
      "prepare",
      "implement",
      "review",
      "verify",
      "summary",
    ]);
    expect(research.phases.map((phase) => phase.id)).toEqual([
      "prepare",
      "investigate",
      "critique",
      "report",
    ]);
  });

  it("keeps the blank preset independent of source-control inputs", () => {
    const blank = missionRecipePreset("blank");

    expect(blank.inputs.map((input) => input.path)).toEqual(["request.goal"]);
    expect(validateMissionRecipe(blank)).toEqual([]);
  });

  it("compiles a result-routed phase with only declared canonical targets", () => {
    const draft = missionRecipePreset("coding");
    const review = draft.phases.find((phase) => phase.id === "review")!;
    const source = missionPhaseSource(draft, review);

    expect(source).toContain("std.encoding.parse_json(phase_result.response.result)");
    expect(source).toContain("runinator.missions.coding_mission_implement");
    expect(source).toContain("runinator.missions.coding_mission_verify");
  });

  it("generates isolated Codex phases with durable role slots", () => {
    const draft = missionRecipePreset("coding", "codex");
    const implement = draft.phases.find((phase) => phase.id === "implement")!;
    const review = draft.phases.find((phase) => phase.id === "review")!;

    expect(draft.schemaVersion).toBe(2);
    expect(draft.agentRuntime).toBe("codex");
    expect(implement.action).toBe("codex");
    expect(implement.profile).toBe("codex");
    expect(implement.actionParameters.sandbox).toBe("workspace_write");
    expect(review.actionParameters.sandbox).toBe("read_only");
    expect(missionPhaseSource(draft, review)).toContain('session_slot: "review"');
    expect(missionPhaseSource(draft, review)).toContain("output_schema:");
  });

  it("switches agent runtimes without discarding recipe edits", () => {
    const draft = missionRecipePreset("coding");
    draft.name = "Release readiness";
    draft.key = "release_readiness";
    draft.inputs[0].description = "A carefully edited objective.";
    draft.phases[1].prompt = "Use the customized implementation prompt.";

    const codex = switchMissionAgentRuntime(draft, "codex");
    const implement = codex.phases.find((phase) => phase.id === "implement")!;

    expect(codex.name).toBe("Release readiness");
    expect(codex.key).toBe("release_readiness");
    expect(codex.inputs[0].description).toBe("A carefully edited objective.");
    expect(implement.prompt).toBe("Use the customized implementation prompt.");
    expect(implement.action).toBe("codex");
    expect(implement.profile).toBe("codex");
    expect(implement.actionParameters.sandbox).toBe("workspace_write");

    const claude = switchMissionAgentRuntime(codex, "claude");
    expect(claude.name).toBe("Release readiness");
    expect(claude.phases[1].action).toBe("claude_code");
    expect(claude.phases[1].profile).toBe("claude");
  });

  it("migrates version one mission drafts to Claude", () => {
    const legacy = missionRecipePreset("blank") as unknown as Record<string, unknown>;
    legacy.schemaVersion = 1;
    delete legacy.agentRuntime;
    const migrated = missionRecipeFromPipeline({
      metadata: { mission_authoring: legacy },
    } as never)!;
    expect(migrated.schemaVersion).toBe(2);
    expect(migrated.agentRuntime).toBe("claude");
  });

  it("rejects reserved inputs and routes outside the recipe", () => {
    const draft = missionRecipePreset("blank");
    draft.inputs.push({
      path: "orchestration.binding_id",
      label: "Bad",
      description: "",
      kind: "string",
      required: true,
    });
    draft.phases[0].routeMode = "fixed";
    draft.phases[0].nextPhase = "missing";

    expect(validateMissionRecipe(draft).join(" ")).toMatch(/reserved/);
    expect(validateMissionRecipe(draft).join(" ")).toMatch(/unknown phase/);
  });

  it("rejects input paths that overlap another input or a system field", () => {
    const draft = missionRecipePreset("blank");
    draft.inputs.push({
      path: "request.goal.detail",
      label: "Detail",
      description: "",
      kind: "string",
      required: false,
    });
    draft.inputs.push({
      path: "mission",
      label: "Mission",
      description: "",
      kind: "any",
      required: false,
    });

    const issues = validateMissionRecipe(draft).join(" ");

    expect(issues).toMatch(/overlaps another input/);
    expect(issues).toMatch(/reserved or invalid/);
  });

  it("rejects unreachable phases and graphs without a reachable terminal", () => {
    const draft = missionRecipePreset("coding");
    draft.phases[0].nextPhase = "prepare";

    const issues = validateMissionRecipe(draft).join(" ");

    expect(issues).toMatch(/unreachable/);
    expect(issues).toMatch(/cannot reach a terminal/);
  });
});
