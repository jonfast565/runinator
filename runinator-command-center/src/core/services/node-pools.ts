import { defaultApi, type NodePoolsApi } from "../api/ports/node-pools";
import type { NodeBackendInfo, ProvisionedGroup, ScaleNodesRequest } from "../api/commandCenterApi";
import type { AppService } from "./app";

export function createNodePoolsService(app: AppService, api: NodePoolsApi = defaultApi) {
  return {
    fetchBackends() {
      return app.runOperation("Loading node backends", () => api.fetchNodeBackends());
    },
    fetchNodes() {
      return app.runOperation("Loading node pools", () => api.fetchNodes());
    },
    scale(request: ScaleNodesRequest) {
      return app.runOperation("Scaling node pool", () => api.scaleNodes(request));
    },
  };
}

export type NodePoolsService = ReturnType<typeof createNodePoolsService>;
export type { NodeBackendInfo, ProvisionedGroup, ScaleNodesRequest };
