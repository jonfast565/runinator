import {
  fetchOrgNodes,
  fetchOrgQuota,
  fetchOrgUsage,
  fetchRateCard,
  scaleOrgNodes,
  type OrgQuota,
  type OrgResourceGroup,
  type OrgUsage,
  type RateCard,
  type ScaleOrgNodesRequest,
} from "../api/commandCenterApi";
import type { AppService } from "./app";

export function createOrgResourcesService(app: AppService) {
  return {
    fetchNodes(orgId: string) {
      return app.runOperation("Loading org nodes", () => fetchOrgNodes(orgId));
    },
    fetchQuota(orgId: string) {
      return fetchOrgQuota(orgId);
    },
    fetchUsage(orgId: string) {
      return fetchOrgUsage(orgId);
    },
    fetchRateCard() {
      return fetchRateCard();
    },
    scaleNodes(orgId: string, request: ScaleOrgNodesRequest) {
      return app.runOperation("Scaling org nodes", () => scaleOrgNodes(orgId, request));
    },
  };
}

export type OrgResourcesService = ReturnType<typeof createOrgResourcesService>;
export type { OrgQuota, OrgResourceGroup, OrgUsage, RateCard, ScaleOrgNodesRequest };
