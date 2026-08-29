// Browser host for the portable Rust `runinatorctl` command language.
//
// xterm supplies input and rendering. This module supplies the process-like command surface:
// clap validation, tokenization, help metadata, completion, and multiline readiness all run in the
// same Rust core as the native executable.

import init, { invoke } from "./wasm/runinator_ctl_wasm.js";
// Inline the module URL so the same initialization works in the browser, Tauri, and Vitest's Node
// environment without a test-only parser or filesystem shim.
import wasmUrl from "./wasm/runinator_ctl_wasm_bg.wasm?url&inline";
import type { ConsoleFlags } from "./types";

export interface CtlArgumentSpec {
  label: string;
  help: string;
}

export interface CtlCommandSpec {
  path: string[];
  usage: string;
  summary: string;
  console_local: boolean;
  arguments: CtlArgumentSpec[];
}

export interface CtlCompletion {
  start: number;
  options: string[];
  hint: string | null;
}

export type CtlParsedLine =
  | { kind: "empty" }
  | {
      kind: "command";
      path: string[];
      args: string[];
      raw_args: string[];
      flags: Record<string, string[]>;
      switches: string[];
      json: boolean;
      console_local: boolean;
    };

type Response<T> = { ok: true; value: T } | { ok: false; error: string };

await init({ module_or_path: wasmUrl });

function call(request: object): unknown {
  const response = JSON.parse(invoke(JSON.stringify(request))) as Response<unknown>;

  if (!response.ok) {
    throw new Error(response.error);
  }

  return response.value;
}

export function ctlCatalog(): CtlCommandSpec[] {
  return call({ op: "catalog" }) as CtlCommandSpec[];
}

export function ctlParse(line: string): CtlParsedLine {
  return call({ op: "parse", line }) as CtlParsedLine;
}

export function ctlComplete(line: string): CtlCompletion {
  return call({ op: "complete", line }) as CtlCompletion;
}

export function ctlIsSubmittable(source: string): boolean {
  return call({ op: "is_submittable", source }) as boolean;
}

export function ctlFlags(parsed: Extract<CtlParsedLine, { kind: "command" }>): ConsoleFlags {
  const flags: ConsoleFlags = { ...parsed.flags };

  for (const name of parsed.switches) {
    flags[name] = true;
  }

  return flags;
}
