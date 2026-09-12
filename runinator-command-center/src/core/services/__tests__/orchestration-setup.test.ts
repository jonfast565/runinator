import { describe, expect, it } from "vitest";
import type { Pipeline } from "../../domain/models";
import { defaultPipelineDefaults } from "../../domain/models";
import {
  applyPreset,
  classifyOrchestrationSetup,
  compileOrchestrationSetup,
  defaultSetupDraft,
  mergeSetupMetadata,
} from "../orchestration-setup";

const pipeline: Pipeline = {
  id: "pipeline-id",
  name: "Release Train",
  key: "release_train",
  namespace: "acme.delivery",
  description: null,
  enabled: true,
  graph: {
    version: 1,
    members: [
      { key: "acme.prepare", workflow_id: "prepare", failure_mode: "continue" },
      { key: "acme.finish", workflow_id: "finish", failure_mode: "continue" },
    ],
    links: [],
    joins: {},
  },
  concurrency: { max_concurrent_runs: 1, on_conflict: "queue" },
  defaults: defaultPipelineDefaults(),
  metadata: { unrelated: { retained: true } },
};

describe("orchestration setup", () => {
  it("generates a latest-wins policy with standard controls", () => {
    const draft = applyPreset(pipeline, defaultSetupDraft(pipeline), "latest_wins");
    const compiled = compileOrchestrationSetup(draft);

    expect(compiled.ingress.scope).toBe("orchestration.release_train");
    expect(compiled.ingress.routes).toContainEqual(
      expect.objectContaining({ lifecycle: "active", action: "dispatch", intent: "refresh" }),
    );
    expect(compiled.orchestration.intents).toMatchObject({
      cancel: { priority: 100, effect: "terminate" },
      pause: { priority: 80, effect: "suspend" },
      resume: { priority: 70, effect: "resume" },
      refresh: { priority: 60, effect: "supersede" },
    });
  });

  it("generates bounded mission mappings and shared workspaces", () => {
    const compiled = compileOrchestrationSetup(defaultSetupDraft(pipeline, "mission"));

    expect(compiled.orchestration.max_epochs).toBe(10);
    expect(compiled.orchestration.phases["acme.prepare"]).toMatchObject({
      result: {
        resources_patch: "/resources_patch",
        evidence: "/evidence",
        next_member: "/next_member",
      },
      workspace: {
        scope: "mission-source",
        lease_seconds: 7200,
        reuse: true,
        recovery: "wait",
        requirements: { capability: "git" },
      },
    });
    expect(compiled.orchestration.phases["acme.finish"].result.next_member).toBeUndefined();
  });

  it("preserves unrelated metadata and recognizes an unchanged guided policy", () => {
    const compiled = compileOrchestrationSetup(defaultSetupDraft(pipeline, "run_once"));
    const saved = { ...pipeline, metadata: mergeSetupMetadata(pipeline.metadata, compiled) };

    expect(saved.metadata.unrelated).toEqual({ retained: true });
    expect(classifyOrchestrationSetup(saved)).toBe("run_once");
  });

  it("routes customized policies to the advanced editor", () => {
    const compiled = compileOrchestrationSetup(defaultSetupDraft(pipeline, "run_once"));
    const saved = { ...pipeline, metadata: mergeSetupMetadata(pipeline.metadata, compiled) };
    const policy = saved.metadata.orchestration as Record<string, unknown>;
    policy.extra = true;

    expect(classifyOrchestrationSetup(saved)).toBe("custom");
  });
});
