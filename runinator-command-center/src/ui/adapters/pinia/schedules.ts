import { defineStore } from "pinia";
import { computed } from "vue";
import { schedulesService } from "../../../core/services";
import type { NewFreezeWindow } from "../../../core/domain/models";
import { mirrorServiceState } from "./sync";

export const useSchedulesStore = defineStore("schedules", () => {
  const state = mirrorServiceState(schedulesService);

  return {
    freezeWindows: computed(() => state.value.freezeWindows),
    activeOnly: computed({
      get: () => state.value.activeOnly,
      set: (value) => { schedulesService.setActiveOnly(value); },
    }),
    activeCount: computed(() => schedulesService.activeCount()),
    refreshFreezeWindows: () => schedulesService.refreshFreezeWindows(),
    saveFreezeWindow: (window: NewFreezeWindow, windowId?: string) =>
      schedulesService.saveFreezeWindow(window, windowId),
    removeFreezeWindow: (windowId: string) => schedulesService.removeFreezeWindow(windowId),
  };
});
