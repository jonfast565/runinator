import {
  applyDevPack,
  cancelWorkflowRun,
  createWorkflowRun,
  fetchWorkflowRun,
  inspectDevPack,
  readDevPackFile,
  replayWorkflowRun,
  writeDevPackFile,
} from "../api/commandCenterApi";
import type { AppService } from "./app";

export function createDevPackService(app: AppService) {
  return {
    inspect(path: string, skipSettings = false) {
      return app.runOperation("Inspecting dev pack", () => inspectDevPack(path, skipSettings));
    },
    readFile(path: string) {
      return app.runOperation("Reading dev pack file", () => readDevPackFile(path));
    },
    writeFile(path: string, contents: string) {
      return app.runOperation("Writing dev pack file", () => writeDevPackFile(path, contents));
    },
    apply(path: string, skipSettings = false) {
      return app.runOperation("Applying dev pack", () => applyDevPack(path, skipSettings));
    },
    createRun(
      workflowId: string,
      options: { debug?: boolean; parameters?: unknown } = {},
    ) {
      return app.runOperation("Starting workflow run", () => createWorkflowRun(workflowId, options));
    },
    fetchRun(runId: string) {
      return app.runOperation("Loading workflow run", () => fetchWorkflowRun(runId));
    },
    cancelRun(runId: string) {
      return app.runOperation("Canceling workflow run", () => cancelWorkflowRun(runId));
    },
    replayRun(workflowRunId: string, options: { fromStepId?: string } = {}) {
      return app.runOperation("Replaying workflow run", () =>
        replayWorkflowRun(workflowRunId, options),
      );
    },
  };
}

export type DevPackService = ReturnType<typeof createDevPackService>;
