import { describe, expect, it } from "vitest";
import type { WorkflowTriggerKindMetadata } from "../../domain/models";
import { validateTriggerEditor } from "../trigger-validation";

const cronMetadata: WorkflowTriggerKindMetadata = {
  kind: "cron",
  label: "Cron",
  icon: "clock",
  description: "Fires on a schedule.",
  fields: [
    {
      name: "cron",
      label: "Schedule",
      ty: { type: "string" },
      required: true,
      secret: false,
      widget: "cron",
    },
  ],
  default_configuration: { cron: "0 * * * *" },
};

const draft = {
  kind: "cron" as const,
  next_execution: null,
  blackout_start: null,
  blackout_end: null,
};

describe("validateTriggerEditor", () => {
  it("requires catalog fields and valid JSON", () => {
    const validation = validateTriggerEditor(draft, "[]", "{", cronMetadata);

    expect(validation.errors.configuration).toBe("Configuration must be a JSON object.");
    expect(validation.errors.metadata).toBe("Metadata must be a JSON object.");
    expect(validation.error).toBe("Configuration must be a JSON object.");
  });

  it("checks invalid cron fields before submitting", () => {
    const validation = validateTriggerEditor(draft, '{"cron":"0 99 * * *"}', "{}", cronMetadata);

    expect(validation.errors.fields.cron).toMatch(/Hour/);
  });

  it("requires a complete, ordered blackout window", () => {
    const partial = validateTriggerEditor(
      { ...draft, blackout_start: "2026-08-31T12:00" },
      '{"cron":"0 * * * *"}',
      "{}",
      cronMetadata,
    );
    const reversed = validateTriggerEditor(
      {
        ...draft,
        blackout_start: "2026-08-31T12:00",
        blackout_end: "2026-08-31T11:00",
      },
      '{"cron":"0 * * * *"}',
      "{}",
      cronMetadata,
    );

    expect(partial.errors.blackoutStart).toBe("Set both the blackout start and end.");
    expect(reversed.errors.blackoutEnd).toBe("Blackout end must be after blackout start.");
  });
});
