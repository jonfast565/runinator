import { describe, expect, it } from "vitest";
import { newTaskDraft } from "../../stores/tasks";
import { isWorkflowTask, validateTask } from "../tasks";

describe("task validation", () => {
  it("requires core task fields", () => {
    const task = newTaskDraft();
    expect(validateTask(task, { default_parameters: "{}", metadata: "{}" })).toBe("Name is required");
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
    expect(validateTask(task, { default_parameters: "{}", metadata: "{}" })).toBe("");
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
    expect(validateTask(task, { default_parameters: "[]", metadata: "{}" })).toContain("JSON objects");
  });

  it("identifies workflow-only task records", () => {
    expect(isWorkflowTask({ ...newTaskDraft(), metadata: { task_type: "workflow" } })).toBe(true);
    expect(isWorkflowTask({ ...newTaskDraft(), metadata: { task_type: "scheduled" } })).toBe(false);
    expect(isWorkflowTask(newTaskDraft())).toBe(false);
  });
});
