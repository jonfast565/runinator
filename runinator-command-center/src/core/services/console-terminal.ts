// the console tab's terminal: a transcript, a line of input, and what each line turned into.
//
// the split is the same one the console itself makes. a bare line is REXRAP and becomes a durable
// cell, evaluated in process when it is pure and run as a workflow when it is not; a `:` line is a
// command, which is answered here and now and leaves nothing behind. only the first kind is part of
// the session's history, which is why a command never appears in `:history`.

import { executeCommand } from "../console/execute";
import { ConsoleParseError } from "../console/tokenize";
import type { ConsoleOutput, ConsoleSessionPort } from "../console/types";
import { createStore } from "./event-bus";
import type { ConsoleService } from "./console";

export type TranscriptStatus = "running" | "ok" | "error";

export interface TranscriptEntry {
  id: string;
  /// what was typed, echoed back above the output the way a terminal does.
  input: string;
  kind: "cell" | "command";
  status: TranscriptStatus;
  outputs: ConsoleOutput[];
  /// set for a cell, so the view can follow the durable cell's own status and result.
  cellId: string | null;
  error: string | null;
}

export interface ConsoleTerminalState {
  entries: TranscriptEntry[];
  /// submitted lines, oldest first, for arrow-up recall.
  history: string[];
  /// true while a line is still being answered.
  busy: boolean;
}

// the transcript is bounded: a console left open all day must not grow without limit, and the
// durable history is in the session anyway.
const MAX_ENTRIES = 400;
const MAX_HISTORY = 200;

export function createConsoleTerminalService(consoleService: ConsoleService) {
  const store = createStore<ConsoleTerminalState>({ entries: [], history: [], busy: false });
  let controller: AbortController | null = null;
  let counter = 0;

  const session: ConsoleSessionPort = {
    current: () => consoleService.getState().activeSession,
    list: () => consoleService.getState().sessions,
    refresh: () => consoleService.refreshSessions(),
    open: (sessionId) => consoleService.openSession(sessionId),
    create: (name) => consoleService.newSession(name),
    remove: async (sessionId) => {
      await consoleService.removeSession(sessionId, {
        // the terminal already asked by making the operator type the command.
        confirm: () => true,
        prompt: () => null,
      });
    },
    cells: () => consoleService.getState().activeSession?.cells ?? [],
    cancelCell: (cellId) => consoleService.cancelCell(cellId),
    replayCell: (cellId) => consoleService.replayCell(cellId),
  };

  function append(entry: TranscriptEntry) {
    store.setState((state) => ({
      ...state,
      entries: [...state.entries, entry].slice(-MAX_ENTRIES),
    }));
  }

  function update(entryId: string, patch: Partial<TranscriptEntry>) {
    store.setState((state) => ({
      ...state,
      entries: state.entries.map((entry) =>
        entry.id === entryId ? { ...entry, ...patch } : entry,
      ),
    }));
  }

  function print(entryId: string, output: ConsoleOutput) {
    store.setState((state) => ({
      ...state,
      entries: state.entries.map((entry) =>
        entry.id === entryId ? { ...entry, outputs: [...entry.outputs, output] } : entry,
      ),
    }));
  }

  function start(input: string, kind: TranscriptEntry["kind"]): TranscriptEntry {
    counter += 1;
    const entry: TranscriptEntry = {
      id: `entry-${String(counter)}`,
      input,
      kind,
      status: "running",
      outputs: [],
      cellId: null,
      error: null,
    };
    append(entry);
    return entry;
  }

  // a command's failure is shown where it happened rather than in the app-wide error banner: the
  // transcript is the log, and a banner would detach the message from the line that caused it.
  function fail(entryId: string, error: unknown) {
    update(entryId, {
      status: "error",
      error: error instanceof Error ? error.message : String(error),
    });
  }

  async function runCommand(entry: TranscriptEntry, line: string) {
    controller = new AbortController();

    try {
      await executeCommand(line, {
        session,
        terminal: {
          clear: () => {
            service.clear();
          },
        },
        signal: controller.signal,
        print: (output) => {
          print(entry.id, output);
        },
      });
      update(entry.id, { status: controller.signal.aborted ? "error" : "ok" });

      if (controller.signal.aborted) {
        update(entry.id, { error: "stopped" });
      }
    } catch (error) {
      fail(entry.id, error);
    } finally {
      controller = null;
    }
  }

  // a REXRAP line becomes a durable cell. the run itself is followed by the console service, which the
  // view reads through `cellId` — so a cell that takes a minute keeps updating in place.
  async function runCell(entry: TranscriptEntry, source: string) {
    if (!consoleService.getState().activeSession) {
      await consoleService.newSession("scratch");
    }

    const created = await consoleService.addCell(source);

    if (!created) {
      throw new Error("no console session is open");
    }

    update(entry.id, { cellId: created.id });
    const ran = await consoleService.runCell(created.id);
    update(entry.id, { status: ran.status === "failed" ? "error" : "ok" });
  }

  const service = {
    ...store,
    /// submit one line: REXRAP, or a `:` command.
    async submit(line: string) {
      const trimmed = line.trim();

      if (!trimmed || store.getState().busy) {
        return;
      }

      store.setState((state) => ({
        ...state,
        busy: true,
        history: [...state.history.filter((entry) => entry !== trimmed), trimmed].slice(
          -MAX_HISTORY,
        ),
      }));

      const command = trimmed.startsWith(":");
      const entry = start(trimmed, command ? "command" : "cell");

      try {
        if (command) {
          await runCommand(entry, trimmed.slice(1));
        } else {
          await runCell(entry, trimmed);
        }
      } catch (error) {
        // a parse error is the operator's typo, not a failure of the console, so it reads as the
        // one-line message rather than as a stack.
        fail(entry.id, error instanceof ConsoleParseError ? error.message : error);
      } finally {
        store.setState((state) => ({ ...state, busy: false }));
      }
    },
    /// stop whatever is running, which is what a terminal's Ctrl+C does.
    stop() {
      controller?.abort();
    },
    clear() {
      store.setState((state) => ({ ...state, entries: [] }));
    },
    /// the transcript's own history, for arrow-up recall.
    history(): string[] {
      return store.getState().history;
    },
    reset() {
      controller?.abort();
      store.setState(() => ({ entries: [], history: [], busy: false }));
    },
  };

  return service;
}

export type ConsoleTerminalService = ReturnType<typeof createConsoleTerminalService>;
