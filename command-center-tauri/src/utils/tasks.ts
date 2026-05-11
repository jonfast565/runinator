import type { ScheduledTask } from "../types/models";
import { parseRequiredObject } from "./json";

export function validateTask(task: ScheduledTask, json: { default_parameters: string; metadata: string }): string {
  if (!task.name.trim()) return "Name is required";
  if (!task.cron_schedule.trim()) return "Cron is required";
  if (!task.action_name.trim()) return "Action name is required";
  if (!task.action_function.trim()) return "Action function is required";
  if (task.timeout <= 0) return "Timeout must be > 0";
  for (const [label, value] of [
    ["parameters", json.default_parameters],
    ["metadata", json.metadata]
  ]) {
    if (!parseRequiredObject(value)) return `${label} and metadata must be JSON objects`;
  }
  return "";
}
