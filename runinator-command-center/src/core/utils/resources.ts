import type { JsonRecord } from "../domain/models";
import { asRecord } from "../workflow/index";
import { displayValue } from "./values";

export function genericRecordType(record: JsonRecord, endpoint: string): string {
  const explicit = record.resource_type ?? record.approval_type ?? record.event_type;

  if (explicit) {
    return displayValue(explicit);
  }

  if (endpoint === "external_items") {
    return "external_item";
  }

  if (endpoint === "automation_events") {
    return "event";
  }

  return endpoint.replace(/s$/, "");
}

export function genericRecordSummary(record: JsonRecord): string {
  if (record.provider === "jira") {
    const key = record.external_id ?? record.key ?? "";
    const title = record.title ?? record.summary ?? "";
    return `${displayValue(key)} ${displayValue(title)}`.trim();
  }

  if (record.provider === "github") {
    const title = record.title ?? record.name ?? "";
    const url = record.url ?? record.html_url ?? "";
    return `${displayValue(title)} ${displayValue(url)}`.trim();
  }

  const metadata = asRecord(record.metadata);
  return displayValue(
    record.title ??
      record.prompt ??
      record.message ??
      record.name ??
      metadata.summary ??
      metadata.url,
  );
}
