import type { JsonRecord } from "../types/models";

export function genericRecordType(record: JsonRecord, endpoint: string): string {
  const explicit =
    record.resource_type ??
    record.feedback_type ??
    record.approval_type ??
    record.gate_type ??
    record.workspace_type ??
    record.change_type ??
    record.event_type;
  if (explicit) return String(explicit);
  if (endpoint === "external_items") return "external_item";
  if (endpoint === "automation_events") return "event";
  return endpoint.replace(/s$/, "");
}

export function genericRecordSummary(record: JsonRecord): string {
  if (record.provider === "jira") {
    const key = record.external_id ?? record.key ?? "";
    const title = record.title ?? record.summary ?? "";
    return `${key} ${title}`.trim();
  }
  if (record.provider === "github") {
    const title = record.title ?? record.name ?? "";
    const url = record.url ?? record.html_url ?? "";
    return `${title} ${url}`.trim();
  }
  return record.title ?? record.prompt ?? record.message ?? record.name ?? record.metadata?.summary ?? record.metadata?.url ?? "";
}
