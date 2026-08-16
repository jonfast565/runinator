// the wdl console: a notebook of cells sharing one scope.
// mirrors runinator-models/src/console.rs; the wire names are the contract.

import type { JsonValue } from "../../json";

export interface ConsoleSession {
  id: string;
  org_id?: string | null;
  name: string;
  created_by?: string | null;
  created_at: string;
  updated_at: string;
}

// what the backend classifier decided a cell was. persisted rather than re-derived, so the ui can
// say why a cell did or did not start a run without re-classifying source that may have changed.
export type ConsoleCellKind = "expression" | "do" | "workflow";

export type ConsoleCellStatus = "idle" | "running" | "succeeded" | "failed";

export interface ConsoleCell {
  id: string;
  session_id: string;
  position: number;
  label?: string | null;
  source: string;
  kind?: ConsoleCellKind | null;
  status: ConsoleCellStatus;
  result?: JsonValue | null;
  error?: string | null;
  // set only for a cell that became a scratch workflow run.
  workflow_run_id?: string | null;
  created_at: string;
  updated_at: string;
}

// one name in a session's scope.
export interface ConsoleBinding {
  id: string;
  session_id: string;
  name: string;
  cell_id?: string | null;
  value: JsonValue;
  created_at: string;
  updated_at: string;
}

export interface ConsoleSessionDetail extends ConsoleSession {
  cells?: ConsoleCell[];
  bindings?: ConsoleBinding[];
}

export interface NewConsoleCell {
  source: string;
  label?: string | null;
  position?: number | null;
}

// what an author writes to reach an earlier cell's result: `params.<name>`.
//
// `params` rather than a console-only root because a bare dotted path in wdl already means *node
// output* — `cells.load` would be a reference to a node called `cells`. kept here so the ui's hints
// and the backend's scope cannot describe different things.
export const CELL_SCOPE = "params";

// the name a cell's result binds to: its label, or `cell_<position>` when it has none.
export function cellBindingName(cell: Pick<ConsoleCell, "label" | "position">): string {
  // note this is emptiness, not nullishness: a whitespace-only label trims to `""`, and `??` would
  // hand that back as the binding name. the backend applies the same filter, and the two must agree
  // or the ui would show a name no cell actually binds to.
  const label = cell.label?.trim();

  if (label) {
    return label;
  }

  return `cell_${String(cell.position)}`;
}

// the expression a later cell uses to read this one.
export function cellReference(cell: Pick<ConsoleCell, "label" | "position">): string {
  return `${CELL_SCOPE}.${cellBindingName(cell)}`;
}

// true when a cell is waiting on a scratch workflow run, which is the only state the ui polls.
export function isCellPending(cell: ConsoleCell): boolean {
  return cell.status === "running";
}
