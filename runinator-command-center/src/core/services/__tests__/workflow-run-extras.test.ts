import { describe, expect, it } from "vitest";
import type { WorkflowEffectOutputEvent } from "../../domain/models";
import { activeTerminalInteraction } from "../workflow-run-extras";

function interaction(
  attempt: number,
  sequence: number,
  state: "input_required" | "input_accepted",
): WorkflowEffectOutputEvent {
  return {
    event_id: `${String(attempt)}-${String(sequence)}`,
    effect_id: "effect",
    workflow_run_id: "run",
    continuation_id: "continuation",
    attempt,
    output: {
      type: "terminal_interaction",
      interaction: {
        sequence,
        request_id: "login",
        state,
        prompt: state === "input_required" ? "Code" : null,
      },
    },
    created_at: sequence,
  };
}

describe("activeTerminalInteraction", () => {
  it("returns the latest required prompt until matching acceptance", () => {
    expect(activeTerminalInteraction([interaction(0, 1, "input_required")])?.prompt).toBe(
      "Code",
    );
    expect(
      activeTerminalInteraction([
        interaction(0, 1, "input_required"),
        interaction(0, 2, "input_accepted"),
      ]),
    ).toBeNull();
  });

  it("prefers the newest attempt and sequence over delivery order", () => {
    expect(
      activeTerminalInteraction([
        interaction(1, 2, "input_accepted"),
        interaction(0, 99, "input_required"),
        interaction(1, 1, "input_required"),
      ]),
    ).toBeNull();
  });
});
