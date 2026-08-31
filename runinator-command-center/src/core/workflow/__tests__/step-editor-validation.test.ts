import { describe, expect, it } from "vitest";
import type { JsonRecord, WorkflowNodeKindMetadata } from "../../domain/models";
import { validateStepEditor } from "../step-editor-validation";

const actionMetadata: WorkflowNodeKindMetadata = {
  kind: "action",
  label: "Task",
  icon: "play",
  description: "Runs a task.",
  category: "task",
  protected: false,
  terminal: false,
  addable: true,
  handler_safe: true,
  runnable_entry: true,
  entry_point: false,
  supports_predicate_edges: false,
  fields: [
    {
      name: "provider",
      label: "Provider",
      ty: { type: "string" },
      required: true,
      secret: false,
      location: { base: "action", path: ["provider"] },
    },
  ],
  edge_slots: [],
  default_template: {},
};

const valid = {
  id: "send_release",
  kind: "action",
  timeout_seconds: 30,
  max_attempts: 3,
  backoff_base_seconds: 2,
  backoff_max_seconds: 30,
  jitter: false,
  retry_on: "any",
  nodeDraft: { action: { provider: "webhook" } } as JsonRecord,
};

describe("validateStepEditor", () => {
  it("accepts a complete step editor draft", () => {
    expect(
      validateStepEditor(valid, "send_release", [{ id: "send_release" }], actionMetadata).error,
    ).toBe("");
  });

  it("shows structural errors before applying a step", () => {
    const validation = validateStepEditor(
      {
        ...valid,
        id: "existing",
        timeout_seconds: -1,
        backoff_max_seconds: 1,
        nodeDraft: { action: { provider: "" } },
      },
      "send_release",
      [{ id: "send_release" }, { id: "existing" }],
      actionMetadata,
    );

    expect(validation.errors.id).toBe("Step ID existing already exists.");
    expect(validation.errors.timeout).toBe("Node timeout must be a whole number of at least 0.");
    expect(validation.errors.retry).toBe("Retry backoff max must be at least the base delay.");
    expect(validation.errors.fields.provider).toBe("Provider is required.");
  });
});
