import { defineStore } from "pinia";
import { computed } from "vue";
import { workspacesService } from "../../../core/services";
import { mirrorServiceState } from "./sync";
export const useWorkspacesStore = defineStore("workspaces", () => {
  const state = mirrorServiceState(workspacesService);
  return {
    items: computed(() => state.value.items),
    selected: computed(() => state.value.selected),
    versions: computed(() => state.value.versions),
    refresh: (offset = 0) => workspacesService.refresh(offset),
    select: (selected: Parameters<typeof workspacesService.select>[0], offset = 0) =>
      workspacesService.select(selected, offset),
    remove: (id: string, version: number | null = null) => workspacesService.remove(id, version),
    download: workspacesService.download,
    clear: () => {
      workspacesService.clear();
    },
  };
});
