import {
  createResourceGrant,
  listResourceGrants,
  revokeResourceGrant,
  setWorkflowOwner,
} from "../api/commandCenterApi";
import type { PermissionLevel, PrincipalType } from "../domain/models";
import type { AppService } from "./app";

export function createWorkflowSharingService(app: AppService) {
  return {
    listGrants(workflowId: string) {
      return app.runOperation("Loading workflow grants", () => listResourceGrants("workflow", workflowId));
    },
    createGrant(
      workflowId: string,
      principalType: PrincipalType,
      principalId: string,
      permission: PermissionLevel,
    ) {
      return app.runOperation("Granting workflow access", () =>
        createResourceGrant("workflow", workflowId, principalType, principalId, permission),
      );
    },
    revokeGrant(workflowId: string, grantId: string) {
      return app.runOperation("Revoking workflow access", () =>
        revokeResourceGrant("workflow", workflowId, grantId),
      );
    },
    setOwner(workflowId: string, orgId: string | null) {
      return app.runOperation("Updating workflow owner", () => setWorkflowOwner(workflowId, orgId));
    },
  };
}

export type WorkflowSharingService = ReturnType<typeof createWorkflowSharingService>;
