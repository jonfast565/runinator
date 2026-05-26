import type { ScheduledTask } from "../types/models";
import { parseRequiredObject } from "./json";

export function validateTask(task: ScheduledTask, json: { configuration: string }): string {
  if (!task.name.trim()) return "Name is required";
  if (!task.cron_schedule.trim()) return "Cron is required";
  if (!task.action_name.trim()) return "Action name is required";
  if (!task.action_function.trim()) return "Action function is required";
  if (task.timeout <= 0) return "Timeout must be > 0";
  if (!parseRequiredObject(json.configuration)) return "Configuration must be a JSON object";
  return "";
}

export function isWorkflowTask(task: ScheduledTask): boolean {
  return task.configuration?.task_type === "workflow";
}
