import type {
  JsonRecord,
  UiField,
  WorkflowTrigger,
  WorkflowTriggerKindMetadata,
} from "../domain/models";
import { parseRequiredObject } from "../utils/json";
import { splitCron, validateCron } from "./cron";

export interface TriggerEditorErrors {
  configuration: string;
  metadata: string;
  nextExecution: string;
  blackoutStart: string;
  blackoutEnd: string;
  fields: Record<string, string>;
}

export interface TriggerEditorValidation {
  configuration: JsonRecord | null;
  metadata: JsonRecord | null;
  errors: TriggerEditorErrors;
  error: string;
}

export function validateTriggerEditor(
  draft: Pick<WorkflowTrigger, "kind" | "next_execution" | "blackout_start" | "blackout_end">,
  configurationText: string,
  metadataText: string,
  kindMetadata?: WorkflowTriggerKindMetadata,
): TriggerEditorValidation {
  const configuration = parseRequiredObject(configurationText);
  const metadata = parseRequiredObject(metadataText);
  const fields = requiredFieldErrors(configuration, kindMetadata?.fields ?? []);
  const errors: TriggerEditorErrors = {
    configuration: configuration ? "" : "Configuration must be a JSON object.",
    metadata: metadata ? "" : "Metadata must be a JSON object.",
    nextExecution: dateTimeError(draft.next_execution, "Next execution"),
    blackoutStart: "",
    blackoutEnd: "",
    fields,
  };

  if (configuration && draft.kind === "cron") {
    const cron = configuration.cron;

    if (typeof cron === "string" && splitCron(cron)) {
      errors.fields.cron = validateCron(cron) ?? "";
    }
  }

  const blackout = blackoutErrors(draft.blackout_start, draft.blackout_end);
  errors.blackoutStart = blackout.start;
  errors.blackoutEnd = blackout.end;

  const error = firstError(errors);
  return { configuration, metadata, errors, error };
}

function requiredFieldErrors(
  configuration: JsonRecord | null,
  fields: UiField[],
): Record<string, string> {
  const errors: Record<string, string> = {};

  if (!configuration) {
    return errors;
  }

  for (const field of fields) {
    if (!field.required || present(configuration[field.name])) {
      continue;
    }

    errors[field.name] = `${field.label ?? field.name} is required.`;
  }

  return errors;
}

function present(value: unknown): boolean {
  return (
    value !== null && value !== undefined && (typeof value !== "string" || Boolean(value.trim()))
  );
}

function dateTimeError(value: string | null | undefined, label: string): string {
  return value && Number.isNaN(new Date(value).getTime())
    ? `${label} must be a valid date and time.`
    : "";
}

function blackoutErrors(
  start: string | null | undefined,
  end: string | null | undefined,
): { start: string; end: string } {
  if (!start && !end) {
    return { start: "", end: "" };
  }

  if (!start || !end) {
    return {
      start: "Set both the blackout start and end.",
      end: "Set both the blackout start and end.",
    };
  }

  const startDate = new Date(start);
  const endDate = new Date(end);

  if (Number.isNaN(startDate.getTime()) || Number.isNaN(endDate.getTime())) {
    return {
      start: Number.isNaN(startDate.getTime())
        ? "Blackout start must be a valid date and time."
        : "",
      end: Number.isNaN(endDate.getTime()) ? "Blackout end must be a valid date and time." : "",
    };
  }

  if (startDate >= endDate) {
    return { start: "", end: "Blackout end must be after blackout start." };
  }

  return { start: "", end: "" };
}

function firstError(errors: TriggerEditorErrors): string {
  const fieldError = Object.values(errors.fields).find(Boolean) ?? "";

  return (
    errors.configuration ||
    errors.metadata ||
    errors.nextExecution ||
    errors.blackoutStart ||
    errors.blackoutEnd ||
    fieldError
  );
}
