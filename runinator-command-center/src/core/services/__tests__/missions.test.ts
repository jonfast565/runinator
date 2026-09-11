import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Pipeline } from "../../domain/models";

vi.mock("../../api/commandCenterApi", () => ({
  admitPipelineIngress: vi.fn(),
  controlWorkflowEffectTerminal: vi.fn(),
  fetchOrchestration: vi.fn(),
  fetchOrchestrationEpochs: vi.fn(),
  fetchOrchestrationEvidence: vi.fn(),
  fetchOrchestrations: vi.fn(),
  fetchPipelineRun: vi.fn(),
  fetchPipelines: vi.fn(),
  fetchWorkflowEffectOutput: vi.fn(),
  fetchWorkflowEffects: vi.fn(),
  sendOrchestrationIntent: vi.fn(),
}));

import { admitPipelineIngress, controlWorkflowEffectTerminal } from "../../api/commandCenterApi";
import { isMissionPipeline, sendMissionSteering, startMission } from "../missions";

const codingPipeline = {
  id: "pipeline-id",
  name: "Bounded Coding Mission",
  description: null,
  enabled: true,
  graph: { version: 1, members: [], links: [], joins: {} },
  concurrency: { max_concurrent_runs: 1, on_conflict: "queue" },
  defaults: {
    on_step_failure: "halt",
    links_enabled_by_default: true,
    default_parameters: {},
    max_chain_depth: null,
    default_failure_mode: "continue",
  },
  metadata: { ingress: { scope: "mission.coding" } },
} as Pipeline;

describe("missions", () => {
  beforeEach(() => vi.clearAllMocks());

  it("recognizes only mission-scoped recipes", () => {
    expect(isMissionPipeline(codingPipeline)).toBe(true);
    expect(
      isMissionPipeline({
        ...codingPipeline,
        metadata: { ingress: { scope: "ticket.lifecycle" } },
      }),
    ).toBe(false);
  });

  it("records kind and correlation in the durable start event", async () => {
    vi.mocked(admitPipelineIngress).mockResolvedValue({
      admission_id: "admission-id",
      generation: 1,
      disposition: "started",
      duplicate: false,
      queue_position: null,
      workflow_run_id: null,
      pipeline_run_id: null,
      orchestration_binding_id: "binding-id",
      message: "accepted",
    });

    await startMission({
      pipelineId: "pipeline-id",
      kind: "coding",
      correlationKey: "feature-123",
      parameters: { request: { goal: "Add a mission dashboard" }, mission: { mcp_config: "/mcp" } },
    });

    expect(admitPipelineIngress).toHaveBeenCalledWith(
      "pipeline-id",
      expect.objectContaining({
        source: "runinator.command_center",
        eventType: "start",
        correlationKey: "feature-123",
        payload: {
          request: { goal: "Add a mission dashboard" },
          mission: {
            kind: "coding",
            correlation_key: "feature-123",
            requested_by: "command_center",
            mcp_config: "/mcp",
          },
        },
      }),
    );
  });

  it("uses structured terminal input for harness steering", async () => {
    await sendMissionSteering("effect-id", "Focus on the failing test");
    expect(controlWorkflowEffectTerminal).toHaveBeenCalledWith("effect-id", {
      type: "input",
      data: "Focus on the failing test",
    });
  });
});
