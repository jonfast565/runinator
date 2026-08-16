import {
  cancelConsoleCell,
  createConsoleCell,
  createConsoleSession,
  deleteConsoleCell,
  deleteConsoleSession,
  fetchConsoleCell,
  fetchConsoleSession,
  fetchConsoleSessions,
  renameConsoleSession,
  replayConsoleCell,
  runConsoleCell,
  updateConsoleCell,
} from "../api/commandCenterApi";
import type { ConsoleCell, ConsoleSession, ConsoleSessionDetail } from "../domain/models";
import { isCellPending } from "../domain/models";
import { createStore } from "./event-bus";
import type { AppService } from "./app";
import type { ConfirmContext } from "./operation-context";

export interface ConsoleState {
  sessions: ConsoleSession[];
  activeSession: ConsoleSessionDetail | null;
  // cells whose scratch run is still in flight, so the view can show them as busy without
  // re-deriving that from status strings.
  pendingCellIds: string[];
}

/// how often a cell waiting on a scratch run is re-read.
const POLL_INTERVAL_MS = 1000;
/// how long that polling continues before giving up and leaving the cell for a manual refresh.
/// bounded so a wedged run cannot leave a timer running for the life of the tab.
const POLL_TIMEOUT_MS = 5 * 60 * 1000;

export function createConsoleService(app: AppService) {
  const store = createStore<ConsoleState>({
    sessions: [],
    activeSession: null,
    pendingCellIds: [],
  });

  function markPending(cellId: string, pending: boolean) {
    store.setState((state) => ({
      ...state,
      pendingCellIds: pending
        ? [...new Set([...state.pendingCellIds, cellId])]
        : state.pendingCellIds.filter((id) => id !== cellId),
    }));
  }

  function replaceCell(cell: ConsoleCell) {
    store.setState((state) => {
      if (!state.activeSession) {
        return state;
      }

      const cells = (state.activeSession.cells ?? []).map((existing) =>
        existing.id === cell.id ? cell : existing,
      );
      return { ...state, activeSession: { ...state.activeSession, cells } };
    });
  }

  // follow one cell's scratch run to a terminal state. re-reading is what settles it: the backend
  // attributes the finished run back to the cell when asked, so a poll never shows `running`
  // forever — but the poll is still bounded, since a wedged run must not leave a timer behind.
  async function followCell(cellId: string) {
    markPending(cellId, true);
    const deadline = Date.now() + POLL_TIMEOUT_MS;

    try {
      while (Date.now() < deadline) {
        await new Promise((resolve) => setTimeout(resolve, POLL_INTERVAL_MS));
        const cell = await fetchConsoleCell(cellId).catch(() => null);

        if (!cell) {
          return;
        }

        replaceCell(cell);

        if (!isCellPending(cell)) {
          // the scope may have gained a binding, so the session is re-read once at the end rather
          // than on every tick.
          await service.refreshActiveSession();
          return;
        }
      }
    } finally {
      markPending(cellId, false);
    }
  }

  const service = {
    ...store,
    isPending(cellId: string): boolean {
      return store.getState().pendingCellIds.includes(cellId);
    },
    async refreshSessions() {
      const sessions = await app
        .runOperation("Refreshing console sessions", fetchConsoleSessions)
        .catch(() => []);
      store.setState((state) => ({ ...state, sessions }));

      const activeId = store.getState().activeSession?.id;
      const next = sessions.find((session) => session.id === activeId) ?? sessions.at(0);

      if (next) {
        await service.openSession(next.id);
        return;
      }

      store.setState((state) => ({ ...state, activeSession: null }));
    },
    async refreshActiveSession() {
      const activeId = store.getState().activeSession?.id;

      if (!activeId) {
        return;
      }

      const detail = await fetchConsoleSession(activeId).catch(() => null);

      if (detail) {
        store.setState((state) => ({ ...state, activeSession: detail }));
      }
    },
    async openSession(sessionId: string) {
      const detail = await app
        .runOperation("Opening console session", () => fetchConsoleSession(sessionId))
        .catch(() => null);
      store.setState((state) => ({ ...state, activeSession: detail }));

      // a session reopened while a cell was still running picks the follow back up, so a reload
      // does not strand a cell showing `running` with nothing watching it.
      for (const cell of detail?.cells ?? []) {
        if (isCellPending(cell)) {
          void followCell(cell.id);
        }
      }
    },
    async newSession(name?: string) {
      const session = await app.runOperation("Creating console session", () =>
        createConsoleSession(name),
      );
      await service.refreshSessions();
      await service.openSession(session.id);
      return session;
    },
    async renameSession(sessionId: string, name: string) {
      await app.runOperation("Renaming console session", () =>
        renameConsoleSession(sessionId, name),
      );
      await service.refreshSessions();
    },
    async removeSession(sessionId: string, confirm: ConfirmContext) {
      if (!confirm.confirm("Delete this console session, its cells, and its scope?")) {
        return;
      }

      await app
        .runOperation("Deleting console session", () => deleteConsoleSession(sessionId))
        .catch((error: unknown) => {
          app.setError(String(error));
        });
      store.setState((state) => ({ ...state, activeSession: null }));
      await service.refreshSessions();
    },
    async addCell(source: string, label?: string | null) {
      const sessionId = store.getState().activeSession?.id;

      if (!sessionId) {
        app.setError("No console session open");
        return;
      }

      await app.runOperation("Adding cell", () => createConsoleCell(sessionId, source, label));
      await service.refreshActiveSession();
    },
    async editCell(cellId: string, source: string, label?: string | null) {
      const cell = await app.runOperation("Saving cell", () =>
        updateConsoleCell(cellId, source, label),
      );
      replaceCell(cell);
    },
    async removeCell(cellId: string, confirm: ConfirmContext) {
      if (!confirm.confirm("Delete this cell and the binding it produced?")) {
        return;
      }

      await app
        .runOperation("Deleting cell", () => deleteConsoleCell(cellId))
        .catch((error: unknown) => {
          app.setError(String(error));
        });
      await service.refreshActiveSession();
    },
    async runCell(cellId: string) {
      const cell = await app.runOperation("Running cell", () => runConsoleCell(cellId));
      replaceCell(cell);

      // a pure cell has already settled by the time this returns; only an effectful one needs
      // following. that asymmetry is the console's whole point, so the ui does not poll for the
      // common case.
      if (isCellPending(cell)) {
        void followCell(cell.id);
      } else {
        await service.refreshActiveSession();
      }

      return cell;
    },
    async cancelCell(cellId: string) {
      await app.runOperation("Canceling cell", () => cancelConsoleCell(cellId));
      markPending(cellId, false);
      await service.refreshActiveSession();
    },
    async replayCell(cellId: string) {
      const cell = await app.runOperation("Replaying cell", () => replayConsoleCell(cellId));
      replaceCell(cell);

      if (isCellPending(cell)) {
        void followCell(cell.id);
      } else {
        await service.refreshActiveSession();
      }

      return cell;
    },
    clearConsole() {
      store.setState(() => ({ sessions: [], activeSession: null, pendingCellIds: [] }));
    },
  };

  return service;
}

export type ConsoleService = ReturnType<typeof createConsoleService>;
