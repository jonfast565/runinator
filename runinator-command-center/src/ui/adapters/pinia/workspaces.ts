import { defineStore } from "pinia";
import { computed } from "vue";
import { workspacesService } from "../../../core/services";
import { mirrorServiceState } from "./sync";
export const useWorkspacesStore = defineStore("workspaces", () => {
  const state = mirrorServiceState(workspacesService);
  return {
    items: computed(() => state.value.items),
    selected: computed(() => state.value.selected),
    pinnedSnapshot: computed(() => state.value.pinnedSnapshot),
    versions: computed(() => state.value.versions),
    directories: computed(() => state.value.directories),
    expandDirectory: (...args: Parameters<typeof workspacesService.expandDirectory>) =>
      workspacesService.expandDirectory(...args),
    collapseDirectory: (path: string) => {
      workspacesService.collapseDirectory(path);
    },
    directory: computed(() => state.value.directory),
    results: computed(() => state.value.results),
    transfer: computed(() => state.value.transfer),
    importArchive: (...args: Parameters<typeof workspacesService.importArchive>) =>
      workspacesService.importArchive(...args),
    cancelTransfer: (...args: Parameters<typeof workspacesService.cancelTransfer>) =>
      workspacesService.cancelTransfer(...args),
    diff: computed(() => state.value.diff),
    compare: (...args: Parameters<typeof workspacesService.compare>) =>
      workspacesService.compare(...args),
    preview: computed(() => state.value.preview),
    browse: (...args: Parameters<typeof workspacesService.browse>) =>
      workspacesService.browse(...args),
    clearPreview: (...args: Parameters<typeof workspacesService.clearPreview>) => {
      workspacesService.clearPreview(...args);
    },
    previewFile: (...args: Parameters<typeof workspacesService.previewFile>) =>
      workspacesService.previewFile(...args),
    refresh: (offset = 0) => workspacesService.refresh(offset),
    select: (...args: Parameters<typeof workspacesService.select>) =>
      workspacesService.select(...args),
    remove: (id: string, version: number | null = null) => workspacesService.remove(id, version),
    download: (...args: Parameters<typeof workspacesService.download>) =>
      workspacesService.download(...args),
    clear: () => {
      workspacesService.clear();
    },
  };
});
