import { defineStore } from "pinia";
import { computed } from "vue";
import { consoleService } from "../../../core/services";
import { mirrorServiceState } from "./sync";

export const useConsoleStore = defineStore("console", () => {
  const state = mirrorServiceState(consoleService);

  return {
    sessions: computed(() => state.value.sessions),
    activeSession: computed(() => state.value.activeSession),
    cells: computed(() => state.value.activeSession?.cells ?? []),
    // the session's scope, sorted by name so it reads as a stable list rather than reordering on
    // every run.
    bindings: computed(() =>
      [...(state.value.activeSession?.bindings ?? [])].sort((left, right) =>
        left.name.localeCompare(right.name),
      ),
    ),
    isPending: (cellId: string) => state.value.pendingCellIds.includes(cellId),
    refreshSessions: () => consoleService.refreshSessions(),
    openSession: (sessionId: string) => consoleService.openSession(sessionId),
    newSession: (name?: string) => consoleService.newSession(name),
    renameSession: (sessionId: string, name: string) =>
      consoleService.renameSession(sessionId, name),
    removeSession: (sessionId: string) =>
      consoleService.removeSession(sessionId, {
        confirm: (message) => window.confirm(message),
        prompt: (message) => window.prompt(message),
      }),
    addCell: (source: string, label?: string | null) => consoleService.addCell(source, label),
    editCell: (cellId: string, source: string, label?: string | null) =>
      consoleService.editCell(cellId, source, label),
    removeCell: (cellId: string) =>
      consoleService.removeCell(cellId, {
        confirm: (message) => window.confirm(message),
        prompt: (message) => window.prompt(message),
      }),
    runCell: (cellId: string) => consoleService.runCell(cellId),
    clearConsole: () => {
      consoleService.clearConsole();
    },
  };
});
