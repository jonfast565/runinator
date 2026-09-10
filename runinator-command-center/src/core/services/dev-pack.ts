import { defaultApi, type DevPackApi } from "../api/ports/dev-pack";

import type { AppService } from "./app";

export function createDevPackService(app: AppService, api: DevPackApi = defaultApi) {
  return {
    inspect(path: string, skipSettings = false) {
      return app.runOperation("Inspecting dev pack", () => api.inspectDevPack(path, skipSettings));
    },
    readFile(path: string) {
      return app.runOperation("Reading dev pack file", () => api.readDevPackFile(path));
    },
    writeFile(path: string, contents: string) {
      return app.runOperation("Writing dev pack file", () => api.writeDevPackFile(path, contents));
    },
    apply(path: string, skipSettings = false) {
      return app.runOperation("Applying dev pack", () => api.applyDevPack(path, skipSettings));
    },
    createRun(workflowId: string, options: { debug?: boolean; parameters?: unknown } = {}) {
      return app.runOperation("Starting workflow run", () =>
        api.createWorkflowRun(workflowId, options),
      );
    },
    fetchRun(runId: string) {
      return app.runOperation("Loading workflow run", () => api.fetchWorkflowRun(runId));
    },
    cancelRun(runId: string) {
      return app.runOperation("Canceling workflow run", () => api.cancelWorkflowRun(runId));
    },
    replayRun(workflowRunId: string, options: { fromStepId?: string } = {}) {
      return app.runOperation("Replaying workflow run", () =>
        api.replayWorkflowRun(workflowRunId, options),
      );
    },
  };
}

export type DevPackService = ReturnType<typeof createDevPackService>;
