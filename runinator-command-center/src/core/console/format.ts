// Turn API payloads into blocks for the terminal.

import type { JsonValue } from "../domain/json";
import type { ConsoleOutput } from "./types";

export function text(value: string, tone?: "muted" | "error" | "success"): ConsoleOutput {
  return { kind: "text", text: value, tone };
}

export function json(value: unknown): ConsoleOutput {
  return { kind: "json", value: value as JsonValue };
}

export function table(columns: string[], rows: string[][]): ConsoleOutput {
  return { kind: "table", columns, rows };
}

/// a cell value for a table: nulls read as `-` rather than as the word "null".
export function cell(value: unknown): string {
  if (value === null || value === undefined || value === "") {
    return "-";
  }

  if (typeof value === "string") {
    return value;
  }

  if (typeof value === "number" || typeof value === "boolean") {
    return String(value);
  }

  return JSON.stringify(value);
}

/// shorten a value for a column, keeping the head, which is where ids and names differ.
export function truncate(value: unknown, width: number): string {
  const rendered = cell(value);
  return rendered.length > width ? `${rendered.slice(0, width - 1)}…` : rendered;
}

/// timestamps in the terminal are short: the date and the minute, without the timezone noise.
export function time(value: unknown): string {
  if (typeof value !== "string" || !value) {
    return "-";
  }

  const parsed = new Date(value);
  return Number.isNaN(parsed.getTime())
    ? value
    : parsed.toISOString().slice(0, 16).replace("T", " ");
}

/// how a command reports a mutation that returns nothing interesting.
export function done(message: string): ConsoleOutput {
  return text(message, "success");
}
