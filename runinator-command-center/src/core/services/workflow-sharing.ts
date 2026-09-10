import { defaultApi, type WorkflowSharingApi } from "../api/ports/workflow-sharing";

import type { PermissionLevel, PrincipalType } from "../domain/models";
import type { AppService } from "./app";

export function createWorkflowSharingService(
  app: AppService,
  api: WorkflowSharingApi = defaultApi,
) {
  return {
    listGrants(workflowId: string) {
      return app.runOperation("Loading workflow grants", () =>
        api.listResourceGrants("workflow", workflowId),
      );
    },
    createGrant(
      workflowId: string,
      principalType: PrincipalType,
      principalId: string,
      permission: PermissionLevel,
    ) {
      return app.runOperation("Granting workflow access", () =>
        api.createResourceGrant("workflow", workflowId, principalType, principalId, permission),
      );
    },
    revokeGrant(workflowId: string, grantId: string) {
      return app.runOperation("Revoking workflow access", () =>
        api.revokeResourceGrant("workflow", workflowId, grantId),
      );
    },
    setOwner(workflowId: string, orgId: string | null) {
      return app.runOperation("Updating workflow owner", () =>
        api.setWorkflowOwner(workflowId, orgId),
      );
    },
  };
}

export type WorkflowSharingService = ReturnType<typeof createWorkflowSharingService>;
