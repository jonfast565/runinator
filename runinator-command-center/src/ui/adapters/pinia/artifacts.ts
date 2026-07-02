import { defineStore } from "pinia";
import { computed } from "vue";
import {
  downloadArtifactInBrowser,
  pickFileFromBrowser,
  uploadArtifactFromBrowser,
} from "../../../api/commandCenterApi";
import {
  downloadArtifactToPath,
  uploadArtifactFromPath,
} from "../../../core/api/commandCenterApi";
import { isTauriRuntime } from "../../../core/api/runtime";
import { artifactsService } from "../../../core/services";
import { mirrorServiceState } from "./sync";

function artifactsUploadContext() {
  return {
    isDesktop: () => isTauriRuntime(),
    pickFile: () => pickFileFromBrowser(),
    uploadFromBrowser: (runId: string, file: File) =>
      uploadArtifactFromBrowser({ run_id: runId }, file),
    uploadFromPath: (runId: string) => uploadArtifactFromPath({ run_id: runId }),
  };
}

function artifactsDownloadContext() {
  return {
    isDesktop: () => isTauriRuntime(),
    downloadInBrowser: downloadArtifactInBrowser,
    downloadToPath: downloadArtifactToPath,
  };
}

function confirmContext() {
  return {
    confirm: (message: string) => window.confirm(message),
    prompt: (message: string) => window.prompt(message),
  };
}

export const useArtifactsStore = defineStore("artifacts", () => {
  const state = mirrorServiceState(artifactsService);

  return {
    artifacts: computed(() => state.value.artifacts),
    selectedArtifactId: computed({
      get: () => state.value.selectedArtifactId,
      set: (id) => { artifactsService.setSelectedArtifactId(id); },
    }),
    selectedArtifact: computed(() => artifactsService.selectedArtifact()),
    uploadRunId: computed({
      get: () => state.value.uploadRunId,
      set: (runId) => { artifactsService.setUploadRunId(runId); },
    }),
    refreshArtifacts: () => artifactsService.refreshArtifacts(),
    clearArtifacts: () => { artifactsService.clearArtifacts(); },
    promptUploadArtifact: () =>
      artifactsService.promptUploadArtifact(artifactsUploadContext(), confirmContext()),
    promptDownloadArtifact: (artifact: Parameters<typeof artifactsService.promptDownloadArtifact>[0]) =>
      artifactsService.promptDownloadArtifact(artifact, artifactsDownloadContext()),
    removeArtifact: (artifact: Parameters<typeof artifactsService.removeArtifact>[0]) =>
      artifactsService.removeArtifact(artifact, confirmContext()),
  };
});
