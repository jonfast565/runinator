import {
  deliverSignal,
  fetchWorkflowNodeRunArtifacts,
  fetchWorkflowNodeRunChunks,
  fetchWorkflowRunArtifacts,
  resolveWorkflowInput,
} from "../api/commandCenterApi";
import { getPlatformAdapter } from "../platform";
import type { RunArtifact, WorkflowRunArtifact } from "../domain/models";
import type { AppService } from "./app";

export function createWorkflowRunExtrasService(app: AppService) {
  const artifacts = () => getPlatformAdapter().artifacts;

  return {
    fetchNodeRunArtifacts(nodeRunId: string) {
      return app
        .runOperation("Loading node run artifacts", () => fetchWorkflowNodeRunArtifacts(nodeRunId))
        .catch(() => [] as RunArtifact[]);
    },
    fetchRunArtifacts(runId: string) {
      return app
        .runOperation("Loading run artifacts", () => fetchWorkflowRunArtifacts(runId))
        .catch(() => [] as WorkflowRunArtifact[]);
    },
    fetchNodeRunChunks(nodeRunId: string) {
      return app.runOperation("Loading node run log", () => fetchWorkflowNodeRunChunks(nodeRunId));
    },
    async downloadArtifact(artifactId: string, name: string) {
      await app.runOperation(`Downloading ${name}`, async () => {
        if (artifacts().isDesktop()) {
          return artifacts().downloadToPath(artifactId, name);
        }

        await artifacts().downloadInBrowser(artifactId, name);
        return { saved_to: null };
      });
    },
    deliverSignal(workflowRunId: string, name: string, payload: unknown = {}) {
      return app.runOperation(`Sending signal '${name}'`, () =>
        deliverSignal(workflowRunId, name, payload),
      );
    },
    resolveInput(
      nodeRunId: string,
      outputJson: unknown,
      resolvedBy?: string,
      message?: string,
    ) {
      return app.runOperation("Resolving workflow input", () =>
        resolveWorkflowInput(nodeRunId, outputJson, resolvedBy, message),
      );
    },
  };
}

export type WorkflowRunExtrasService = ReturnType<typeof createWorkflowRunExtrasService>;
