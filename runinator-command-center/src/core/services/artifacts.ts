import { deleteArtifact, fetchAllArtifacts } from "../api/commandCenterApi";
import type { RunArtifact } from "../domain/models";
import { createStore } from "./event-bus";
import type { AppService } from "./app";
import type { ConfirmContext } from "./operation-context";

export interface ArtifactsUploadContext {
  isDesktop(): boolean;
  pickFile(): Promise<File | null>;
  uploadFromBrowser(runId: string, file: File): Promise<RunArtifact>;
  uploadFromPath(runId: string): Promise<RunArtifact>;
}

export interface ArtifactsDownloadContext {
  isDesktop(): boolean;
  downloadInBrowser(artifactId: string, name: string): Promise<void>;
  downloadToPath(artifactId: string, name: string): Promise<{ saved_to: string | null }>;
}

export interface ArtifactsState {
  artifacts: RunArtifact[];
  selectedArtifactId: string | null;
  uploadRunId: string;
}

export function createArtifactsService(app: AppService) {
  const store = createStore<ArtifactsState>({
    artifacts: [],
    selectedArtifactId: null,
    uploadRunId: "",
  });

  function selectedArtifact(): RunArtifact | null {
    const { artifacts, selectedArtifactId } = store.getState();
    return artifacts.find((artifact) => artifact.id === selectedArtifactId) ?? null;
  }

  const service = {
    ...store,
    selectedArtifact,
    setSelectedArtifactId(id: string | null) {
      store.setState((state) => ({ ...state, selectedArtifactId: id }));
    },
    setUploadRunId(runId: string) {
      store.setState((state) => ({ ...state, uploadRunId: runId }));
    },
    async refreshArtifacts() {
      const artifacts = await app
        .runOperation("Loading artifacts", () => fetchAllArtifacts())
        .catch(() => []);
      store.setState((state) => ({ ...state, artifacts }));
    },
    clearArtifacts() {
      store.setState(() => ({
        artifacts: [],
        selectedArtifactId: null,
        uploadRunId: "",
      }));
    },
    promptForRunId(confirm: ConfirmContext): string | null {
      const value = confirm.prompt("Attach artifact to which run id?");

      if (!value) {
        return null;
      }

      const runId = value.trim();

      if (!runId) {
        app.setError("Invalid run id");
        return null;
      }

      return runId;
    },
    async promptUploadArtifact(upload: ArtifactsUploadContext, confirm: ConfirmContext) {
      const { uploadRunId } = store.getState();
      const result = await app
        .runOperation("Uploading artifact", async () => {
          const runId = uploadRunId.trim() || service.promptForRunId(confirm);

          if (!runId) {
            return null;
          }

          if (upload.isDesktop()) {
            return upload.uploadFromPath(runId);
          }

          const file = await upload.pickFile();

          if (!file) {
            return null;
          }

          return upload.uploadFromBrowser(runId, file);
        })
        .catch((error: unknown) => {
          app.setError(String(error));
          return null;
        });

      if (result) {
        app.setStatus(`Uploaded artifact ${result.name}`);
        await service.refreshArtifacts();
      }
    },
    async promptDownloadArtifact(artifact: RunArtifact, download: ArtifactsDownloadContext) {
      await app
        .runOperation(`Downloading ${artifact.name}`, async () => {
          if (download.isDesktop()) {
            return download.downloadToPath(artifact.id, artifact.name);
          }

          await download.downloadInBrowser(artifact.id, artifact.name);
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
    },
    async removeArtifact(artifact: RunArtifact, confirm: ConfirmContext) {
      if (!confirm.confirm(`Delete artifact "${artifact.name}"? This also removes the stored file.`)) {
        return;
      }

      await app
        .runOperation(`Deleting ${artifact.name}`, () => deleteArtifact(artifact.id))
        .catch((error: unknown) => {
          app.setError(String(error));
        });

      store.setState((state) => ({
        ...state,
        artifacts: state.artifacts.filter((entry) => entry.id !== artifact.id),
        selectedArtifactId:
          state.selectedArtifactId === artifact.id ? null : state.selectedArtifactId,
      }));

      await service.refreshArtifacts();
    },
  };

  return service;
}

export type ArtifactsService = ReturnType<typeof createArtifactsService>;
