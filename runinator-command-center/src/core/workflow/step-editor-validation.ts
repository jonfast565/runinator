import type { JsonRecord, WorkflowNodeKindMetadata } from "../domain/models";
import { isBlankValue } from "../utils/values";
import { getAtLocation } from "./field-location";
import { RETRY_CLASSES, type RetryPolicy } from "./retry";

export interface StepEditorValues extends RetryPolicy {
  id: string;
  kind: string;
  timeout_seconds: number;
  nodeDraft: JsonRecord;
}

export interface StepEditorErrors {
  id: string;
  kind: string;
  timeout: string;
  retry: string;
  fields: Record<string, string>;
}

export interface StepEditorValidation {
  errors: StepEditorErrors;
  error: string;
}

export function validateStepEditor(
  values: StepEditorValues,
  selectedStepId: string,
  nodes: JsonRecord[],
  kindMetadata?: WorkflowNodeKindMetadata,
): StepEditorValidation {
  const id = values.id.trim();
  const errors: StepEditorErrors = {
    id: !id
      ? "Step ID is required."
      : id !== selectedStepId && nodes.some((node) => String(node.id) === id)
        ? `Step ID ${id} already exists.`
        : "",
    kind: values.kind.trim() ? "" : "Choose a node kind.",
    timeout: wholeNumberError(values.timeout_seconds, "Node timeout", 0),
    retry: retryError(values),
    fields: requiredFieldErrors(values.nodeDraft, kindMetadata),
  };

  const fieldError = Object.values(errors.fields).find(Boolean) ?? "";
  const error = errors.id || errors.kind || errors.timeout || errors.retry || fieldError;

  return { errors, error };
}

function wholeNumberError(value: number, label: string, minimum: number): string {
  return Number.isFinite(value) && Number.isInteger(value) && value >= minimum
    ? ""
    : `${label} must be a whole number of at least ${String(minimum)}.`;
}

function retryError(values: RetryPolicy): string {
  const maxAttempts = wholeNumberError(values.max_attempts, "Max attempts", 1);

  if (maxAttempts) {
    return maxAttempts;
  }

  const base = wholeNumberError(values.backoff_base_seconds, "Retry backoff base", 0);

  if (base) {
    return base;
  }

  const max = wholeNumberError(values.backoff_max_seconds, "Retry backoff max", 0);

  if (max) {
    return max;
  }

  if (values.backoff_max_seconds < values.backoff_base_seconds) {
    return "Retry backoff max must be at least the base delay.";
  }

  return RETRY_CLASSES.some((entry) => entry.value === values.retry_on)
    ? ""
    : "Choose when this step should retry.";
}

function requiredFieldErrors(
  nodeDraft: JsonRecord,
  kindMetadata?: WorkflowNodeKindMetadata,
): Record<string, string> {
  const errors: Record<string, string> = {};

  for (const field of kindMetadata?.fields ?? []) {
    if (field.required && isBlankValue(getAtLocation(nodeDraft, field.location))) {
      errors[field.name] = `${field.label ?? field.name} is required.`;
    }
  }

  return errors;
}
