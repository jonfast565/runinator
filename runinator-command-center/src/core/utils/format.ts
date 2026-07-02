export function formatDate(value?: string | null): string {
  if (!value) {
    return "-";
  }

  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
}

export function pretty(value: unknown): string {
  return JSON.stringify(value ?? {}, null, 2);
}

// extract a human-readable message from an unknown thrown value.
export function errorMessage(err: unknown): string {
  if (err instanceof Error) {
    return err.message;
  }

  if (typeof err === "string") {
    return err;
  }

  if (err && typeof err === "object" && "message" in err) {
    const message = (err).message;
    return typeof message === "string" ? message : String(message);
  }

  return String(err);
}

// normalize a run/node error for display: unwrap common json envelopes and trim noise.
export function formatErrorMessage(raw: unknown): string {
  if (raw === null || raw === undefined) {
    return "";
  }

  let text = typeof raw === "string" ? raw : JSON.stringify(raw);
  text = text.trim();

  if (!text) {
    return "";
  }

  const looksJson =
    (text.startsWith("{") && text.endsWith("}")) || (text.startsWith("[") && text.endsWith("]"));

  if (looksJson) {
    try {
      const parsed: unknown = JSON.parse(text);
      const extracted = extractErrorText(parsed);
      return extracted ? extracted.trim() : JSON.stringify(parsed, null, 2);
    } catch {
      // not valid json after all; fall back to the raw text.
    }
  }

  return text;
}

// pull a human message out of an error envelope like {"error":"...","message":"..."}.
function extractErrorText(value: unknown): string {
  if (typeof value === "string") {
    return value;
  }

  if (value && typeof value === "object") {
    for (const key of ["message", "error", "detail", "reason", "description"]) {
      const candidate = (value as Record<string, unknown>)[key];

      if (typeof candidate === "string" && candidate.trim()) {
        return candidate;
      }

      if (candidate && typeof candidate === "object") {
        const nested = extractErrorText(candidate);

        if (nested) {
          return nested;
        }
      }
    }
  }

  return "";
}
