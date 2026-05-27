import { defineStore } from "pinia";
import { computed, ref } from "vue";
import {
  downloadArtifactInBrowser,
  downloadArtifactToPath,
  fetchAllArtifacts,
  pickFileFromBrowser,
  uploadArtifactFromBrowser,
  uploadArtifactFromPath
} from "../api/commandCenterApi";
import { isTauriRuntime } from "../api/tauriRuntime";
import type { RunArtifact } from "../types/models";
import { useAppStore } from "./app";

export const useArtifactsStore = defineStore("artifacts", () => {
  const app = useAppStore();
  const artifacts = ref<RunArtifact[]>([]);
  const selectedArtifactId = ref<number>(0);
  const uploadRunId = ref<number | null>(null);

  const selectedArtifact = computed(() =>
    artifacts.value.find((artifact) => artifact.id === selectedArtifactId.value) ?? null
  );

  async function refreshArtifacts() {
    artifacts.value = await app.runOperation("Loading artifacts", () => fetchAllArtifacts()).catch(() => []);
  }

  function clearArtifacts() {
    artifacts.value = [];
    selectedArtifactId.value = 0;
  }

  async function promptUploadArtifact() {
    const result = await app.runOperation("Uploading artifact", async () => {
      const runId = uploadRunId.value && uploadRunId.value > 0 ? uploadRunId.value : promptForRunId();
      if (!runId) return null;
      if (isTauriRuntime()) return uploadArtifactFromPath({ run_id: runId });
      const file = await pickFileFromBrowser();
      if (!file) return null;
      return uploadArtifactFromBrowser({ run_id: runId }, file);
    }).catch((error) => {
      app.setError(String(error));
      return null;
    });
    if (result) {
      app.setStatus(`Uploaded artifact ${result.name}`);
      await refreshArtifacts();
    }
  }

  function promptForRunId(): number | null {
    const value = window.prompt("Attach artifact to which run id?");
    if (!value) return null;
    const parsed = Number(value);
    if (!Number.isFinite(parsed) || parsed <= 0) {
      app.setError("Invalid run id");
      return null;
    }
    return parsed;
  }

  async function promptDownloadArtifact(artifact: RunArtifact) {
    await app.runOperation(`Downloading ${artifact.name}`, async () => {
      if (isTauriRuntime()) return downloadArtifactToPath(artifact.id, artifact.name);
      await downloadArtifactInBrowser(artifact.id, artifact.name);
      return { saved_to: null };
    }).then((info) => {
      if (info?.saved_to) app.setStatus(`Saved to ${info.saved_to}`);
      else app.setStatus(`Downloaded ${artifact.name}`);
    }).catch((error) => {
      app.setError(String(error));
    });
  }

  return {
    artifacts,
    selectedArtifactId,
    selectedArtifact,
    uploadRunId,
    refreshArtifacts,
    clearArtifacts,
    promptUploadArtifact,
    promptDownloadArtifact
  };
});
