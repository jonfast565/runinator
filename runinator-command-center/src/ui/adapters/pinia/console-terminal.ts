import { defineStore } from "pinia";
import { computed } from "vue";
import { consoleTerminalService } from "../../../core/services";
import { mirrorServiceState } from "./sync";

export const useConsoleTerminalStore = defineStore("console-terminal", () => {
  const state = mirrorServiceState(consoleTerminalService);

  return {
    entries: computed(() => state.value.entries),
    history: computed(() => state.value.history),
    busy: computed(() => state.value.busy),
    submit: (line: string) => consoleTerminalService.submit(line),
    stop: () => {
      consoleTerminalService.stop();
    },
    clear: () => {
      consoleTerminalService.clear();
    },
    reset: () => {
      consoleTerminalService.reset();
    },
  };
});
