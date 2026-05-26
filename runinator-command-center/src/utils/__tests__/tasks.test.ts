import { describe, expect, it } from "vitest";
import { newTaskDraft } from "../../stores/tasks";
import { isWorkflowTask, validateTask } from "../tasks";

describe("task validation", () => {
  it("requires core task fields", () => {
    const task = newTaskDraft();
    expect(validateTask(task, { configuration: "{}" })).toBe("Name is required");
  });

  it("accepts a complete task with object JSON fields", () => {
    const task = {
      ...newTaskDraft(),
      name: "Task",
      cron_schedule: "* * * * *",
      action_name: "provider",
      action_function: "run",
      timeout: 1000
    };
    expect(validateTask(task, { configuration: "{}" })).toBe("");
  });

  it("rejects non-object JSON fields", () => {
    const task = {
      ...newTaskDraft(),
      name: "Task",
      cron_schedule: "* * * * *",
      action_name: "provider",
      action_function: "run",
      timeout: 1000
    };
    expect(validateTask(task, { configuration: "[]" })).toContain("JSON object");
  });

  it("identifies workflow-only task records", () => {
    expect(isWorkflowTask({ ...newTaskDraft(), configuration: { task_type: "workflow" } })).toBe(true);
    expect(isWorkflowTask({ ...newTaskDraft(), configuration: { task_type: "scheduled" } })).toBe(false);
    expect(isWorkflowTask(newTaskDraft())).toBe(false);
  });
});
