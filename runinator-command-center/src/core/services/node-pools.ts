import {
  fetchNodeBackends,
  fetchNodes,
  scaleNodes,
  type NodeBackendInfo,
  type ProvisionedGroup,
  type ScaleNodesRequest,
} from "../api/commandCenterApi";
import type { AppService } from "./app";

export function createNodePoolsService(app: AppService) {
  return {
    fetchBackends() {
      return app.runOperation("Loading node backends", () => fetchNodeBackends());
    },
    fetchNodes() {
      return app.runOperation("Loading node pools", () => fetchNodes());
    },
    scale(request: ScaleNodesRequest) {
      return app.runOperation("Scaling node pool", () => scaleNodes(request));
    },
  };
}

export type NodePoolsService = ReturnType<typeof createNodePoolsService>;
export type { NodeBackendInfo, ProvisionedGroup, ScaleNodesRequest };
