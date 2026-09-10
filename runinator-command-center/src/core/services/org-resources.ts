import { defaultApi, type OrgResourcesApi } from "../api/ports/org-resources";
import type {
  OrgQuota,
  OrgResourceGroup,
  OrgUsage,
  RateCard,
  ScaleOrgNodesRequest,
} from "../api/commandCenterApi";
import type { AppService } from "./app";

export function createOrgResourcesService(app: AppService, api: OrgResourcesApi = defaultApi) {
  return {
    fetchNodes(orgId: string) {
      return app.runOperation("Loading org nodes", () => api.fetchOrgNodes(orgId));
    },
    fetchQuota(orgId: string) {
      return api.fetchOrgQuota(orgId);
    },
    fetchUsage(orgId: string) {
      return api.fetchOrgUsage(orgId);
    },
    fetchRateCard() {
      return api.fetchRateCard();
    },
    scaleNodes(orgId: string, request: ScaleOrgNodesRequest) {
      return app.runOperation("Scaling org nodes", () => api.scaleOrgNodes(orgId, request));
    },
  };
}

export type OrgResourcesService = ReturnType<typeof createOrgResourcesService>;
export type { OrgQuota, OrgResourceGroup, OrgUsage, RateCard, ScaleOrgNodesRequest };
