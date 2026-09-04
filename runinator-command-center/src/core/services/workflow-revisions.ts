import {
  fetchWorkflowRevision,
  fetchWorkflowRevisions,
  restoreWorkflowRevision,
} from "../api/commandCenterApi";
import type { WorkflowRevision } from "../domain/models";
import type { AppService } from "./app";

export function createWorkflowRevisionsService(app: AppService) {
  return {
    /** a workflow's history, newest first. */
    list(workflowId: string, limit?: number): Promise<WorkflowRevision[]> {
      return app.runOperation(
        "Loading revision history",
        () => fetchWorkflowRevisions(workflowId, limit),
        { retryable: true },
      );
    },
    /** one revision including the definition it captured — what a diff reads. */
    get(workflowId: string, revision: number): Promise<WorkflowRevision> {
      return app.runOperation(
        "Loading revision",
        () => fetchWorkflowRevision(workflowId, revision),
        {
          retryable: true,
        },
      );
    },
    /**
     * restore an earlier revision. deliberately not `retryable`: the restore is a write that
     * creates a new revision, so replaying one that failed after it landed would stack a second
     * identical rollback onto the history.
     */
    restore(workflowId: string, revision: number, contractOverrideReason?: string) {
      return app.runOperation("Restoring revision", () =>
        restoreWorkflowRevision(workflowId, revision, contractOverrideReason),
      );
    },
  };
}

export type WorkflowRevisionsService = ReturnType<typeof createWorkflowRevisionsService>;
