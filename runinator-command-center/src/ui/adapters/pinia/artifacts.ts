import { defineStore } from "pinia";
import { computed } from "vue";
import { getPlatformAdapter } from "../../../core/platform";
import { artifactsService } from "../../../core/services";
import { mirrorServiceState } from "./sync";

function artifactsUploadContext() {
  const { artifacts } = getPlatformAdapter();

  return {
    isDesktop: () => artifacts.isDesktop(),
    pickFile: () => artifacts.pickFile(),
    uploadFromBrowser: (runId: string, file: File) =>
      artifacts.uploadFromBrowser({ run_id: runId }, file),
    uploadFromPath: (runId: string) => artifacts.uploadFromPath({ run_id: runId }),
  };
}

function artifactsDownloadContext() {
  const { artifacts } = getPlatformAdapter();

  return {
    isDesktop: () => artifacts.isDesktop(),
    downloadInBrowser: artifacts.downloadInBrowser.bind(artifacts),
    downloadToPath: artifacts.downloadToPath.bind(artifacts),
  };
}

function confirmContext() {
  const { dialogs } = getPlatformAdapter();

  return {
    confirm: dialogs.confirm.bind(dialogs),
    prompt: dialogs.prompt.bind(dialogs),
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
