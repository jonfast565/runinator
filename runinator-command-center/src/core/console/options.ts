// pulling flags off a command line, and reading them back.

import type { ConsoleFlags } from "./types";

export class ConsoleParseError extends Error {}

/// the single value of a flag, or undefined when it was not given.
export function flag(flags: ConsoleFlags, name: string): string | undefined {
  const value = flags[name];
  return Array.isArray(value) ? value.at(-1) : undefined;
}

/// every value of a repeatable flag.
export function flagList(flags: ConsoleFlags, name: string): string[] {
  const value = flags[name];
  return Array.isArray(value) ? value : [];
}

/// true when a flag was present at all.
export function flagSet(flags: ConsoleFlags, name: string): boolean {
  return flags[name] !== undefined;
}

/// a flag's value, or the failure that says which flag was missing.
export function requiredFlag(flags: ConsoleFlags, name: string): string {
  const value = flag(flags, name);

  if (value === undefined) {
    throw new ConsoleParseError(`--${name} is required`);
  }

  return value;
}

/// a positional argument, or the failure that names it.
export function requiredArg(args: string[], index: number, name: string): string {
  const value = args.at(index);

  if (value === undefined || value === "") {
    throw new ConsoleParseError(`${name} is required`);
  }

  return value;
}

/// a flag parsed as a number, rejecting anything that is not one.
export function numberFlag(flags: ConsoleFlags, name: string): number | undefined {
  const value = flag(flags, name);

  if (value === undefined) {
    return undefined;
  }

  const parsed = Number(value);

  if (!Number.isFinite(parsed)) {
    throw new ConsoleParseError(`--${name} must be a number`);
  }

  return parsed;
}

/// `KEY=VALUE` pairs from a repeatable flag, with json values parsed and everything else kept as a
/// string. this is `runinatorctl --param` semantics.
export function keyValueFlags(flags: ConsoleFlags, name: string): Record<string, unknown> {
  const parameters: Record<string, unknown> = {};

  for (const entry of flagList(flags, name)) {
    const separator = entry.indexOf("=");

    if (separator <= 0) {
      throw new ConsoleParseError(`--${name} expects KEY=VALUE, got '${entry}'`);
    }

    const key = entry.slice(0, separator);
    const raw = entry.slice(separator + 1);

    try {
      parameters[key] = JSON.parse(raw);
    } catch {
      parameters[key] = raw;
    }
  }

  return parameters;
}

/// a json argument, reported by name when it does not parse.
export function parseJson(text: string, label: string): unknown {
  try {
    return JSON.parse(text);
  } catch (error) {
    throw new ConsoleParseError(`${label} must be valid json: ${String(error)}`);
  }
}
