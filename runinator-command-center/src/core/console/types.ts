// what a console command is, and what it may say back.
//
// a command never writes to the dom and never touches a pinia store: it reads its arguments, calls
// the api, and prints. that is what keeps the whole command surface testable without mounting the
// terminal.

import type { JsonValue } from "../domain/json";
import type { ConsoleCell, ConsoleSession, ConsoleSessionDetail } from "../domain/models";

/// one block of terminal output.
export type ConsoleOutput =
  | { kind: "text"; text: string; tone?: "muted" | "error" | "success" }
  | { kind: "json"; value: JsonValue }
  | { kind: "table"; columns: string[]; rows: string[][] };

/// flags parsed off a command line. a repeated flag keeps every value, since several commands take
/// a repeatable `--param`/`--label`.
/// a flag absent from the line reads as `undefined`, which is why the value type says so: without
/// it every `flags.x !== undefined` check would look redundant to the type checker.
export type ConsoleFlags = Record<string, string[] | true | undefined>;

/// the session operations the notebook-shaped commands need. supplied by the caller rather than
/// imported, so this module never depends on a store.
export interface ConsoleSessionPort {
  current: () => ConsoleSessionDetail | null;
  list: () => ConsoleSession[];
  refresh: () => Promise<void>;
  open: (sessionId: string) => Promise<void>;
  create: (name?: string) => Promise<ConsoleSession>;
  remove: (sessionId: string) => Promise<void>;
  cells: () => ConsoleCell[];
  /// cancel the durable run behind an effectful cell.
  cancelCell: (cellId: string) => Promise<void>;
  /// run a settled cell again against the session's current scope.
  replayCell: (cellId: string) => Promise<ConsoleCell>;
}

/// the surface a command may clear. the transcript is the terminal's, not the session's, so this is
/// the one thing a command reaches back into the view for.
export interface ConsoleTerminalPort {
  clear: () => void;
}

export interface ConsoleCommandContext {
  /// positional arguments, with the command's own path words already removed.
  args: string[];
  flags: ConsoleFlags;
  /// `--json` asks for the raw payload instead of the formatted table.
  json: boolean;
  /// aborted when the operator stops a running command.
  signal: AbortSignal;
  session: ConsoleSessionPort;
  terminal: ConsoleTerminalPort;
  // a property rather than a method, because commands destructure it off the context and a method
  // torn from its object is exactly what `unbound-method` warns about.
  print: (output: ConsoleOutput) => void;
}

export interface ConsoleCommand {
  /// the words that select this command, e.g. `["workflows", "list"]`.
  path: string[];
  summary: string;
  /// the full call shape, shown by `:help <command>`.
  usage: string;
  /// flags that take no value, so `--open list` does not swallow `list`.
  booleans?: string[];
  /// a command that has nothing to await returns nothing; the dispatcher awaits either shape.
  run: (context: ConsoleCommandContext) => void | Promise<void>;
}

/// a command that cannot work in a browser, kept in the catalog so `:help` still explains it.
export class UnavailableCommandError extends Error {
  constructor(command: string, reason: string) {
    super(`${command} is not available in the web console: ${reason}`);
  }
}
