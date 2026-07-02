import { fetchReplicaSamples, type ReplicaSample, type ReplicaSampleSeries } from "../api/commandCenterApi";
import type { AppService } from "./app";

export function createReplicaSamplesService(app: AppService) {
  return {
    async fetch(replicaId: string, sinceSeconds?: number): Promise<ReplicaSampleSeries> {
      return app
        .runOperation("Loading replica samples", () => fetchReplicaSamples(replicaId, sinceSeconds))
        .catch(() => ({ replica_id: replicaId, samples: [] as ReplicaSample[] }));
    },
  };
}

export type ReplicaSamplesService = ReturnType<typeof createReplicaSamplesService>;
export type { ReplicaSample, ReplicaSampleSeries };
