import { defineStore } from "pinia";
import { computed, ref } from "vue";
import {
  deleteArtifact,
  downloadArtifactInBrowser,
  downloadArtifactToPath,
  fetchAllArtifacts,
  pickFileFromBrowser,
  uploadArtifactFromBrowser,
  uploadArtifactFromPath,
} from "../../../api/commandCenterApi";
import { isTauriRuntime } from "../../../api/tauriRuntime";
import type { RunArtifact } from "../../../types/models";
import { useAppStore } from "./app";

export const useArtifactsStore = defineStore("artifacts", () => {
  const app = useAppStore();
  const artifacts = ref<RunArtifact[]>([]);
  const selectedArtifactId = ref<string | null>(null);
  const uploadRunId = ref<string>("");

  const selectedArtifact = computed(
    () => artifacts.value.find((artifact) => artifact.id === selectedArtifactId.value) ?? null,
  );

  async function refreshArtifacts() {
    artifacts.value = await app
      .runOperation("Loading artifacts", () => fetchAllArtifacts())
      .catch(() => []);
  }

  function clearArtifacts() {
    artifacts.value = [];
    selectedArtifactId.value = null;
  }

  async function promptUploadArtifact() {
    const result = await app
      .runOperation("Uploading artifact", async () => {
        const runId = uploadRunId.value.trim() || promptForRunId();

        if (!runId) {
          return null;
        }

        if (isTauriRuntime()) {
          return uploadArtifactFromPath({ run_id: runId });
        }

        const file = await pickFileFromBrowser();

        if (!file) {
          return null;
        }

        return uploadArtifactFromBrowser({ run_id: runId }, file);
      })
      .catch((error: unknown) => {
        app.setError(String(error));
        return null;
      });

    if (result) {
      app.setStatus(`Uploaded artifact ${result.name}`);
      await refreshArtifacts();
    }
  }

  function promptForRunId(): string | null {
    const value = window.prompt("Attach artifact to which run id?");

    if (!value) {
      return null;
    }

    const runId = value.trim();

    if (!runId) {
      app.setError("Invalid run id");
      return null;
    }

    return runId;
  }

  async function promptDownloadArtifact(artifact: RunArtifact) {
    await app
      .runOperation(`Downloading ${artifact.name}`, async () => {
        if (isTauriRuntime()) {
          return downloadArtifactToPath(artifact.id, artifact.name);
        }

        await downloadArtifactInBrowser(artifact.id, artifact.name);
        return { saved_to: null };
      })
      .then((info) => {
        if (info.saved_to) {
          app.setStatus(`Saved to ${info.saved_to}`);
        } else {
          app.setStatus(`Downloaded ${artifact.name}`);
        }
      })
      .catch((error: unknown) => {
        app.setError(String(error));
      });
  }

  async function removeArtifact(artifact: RunArtifact) {
    if (!window.confirm(`Delete artifact "${artifact.name}"? This also removes the stored file.`)) {
      return;
    }

    await app
      .runOperation(`Deleting ${artifact.name}`, () => deleteArtifact(artifact.id))
      .catch((error: unknown) => {
        app.setError(String(error));
      });
    artifacts.value = artifacts.value.filter((entry) => entry.id !== artifact.id);

    if (selectedArtifactId.value === artifact.id) {
      selectedArtifactId.value = null;
    }

    await refreshArtifacts();
  }

  return {
    artifacts,
    selectedArtifactId,
    selectedArtifact,
    uploadRunId,
    refreshArtifacts,
    clearArtifacts,
    promptUploadArtifact,
    promptDownloadArtifact,
    removeArtifact,
  };
});
