import { defaultApi, type ReplicaSamplesApi } from "../api/ports/replica-samples";
import type { ReplicaSample, ReplicaSampleSeries } from "../api/commandCenterApi";
import type { AppService } from "./app";

export function createReplicaSamplesService(app: AppService, api: ReplicaSamplesApi = defaultApi) {
  return {
    async fetch(replicaId: string, sinceSeconds?: number): Promise<ReplicaSampleSeries> {
      return app
        .runOperation("Loading replica samples", () =>
          api.fetchReplicaSamples(replicaId, sinceSeconds),
        )
        .catch(() => ({ replica_id: replicaId, samples: [] as ReplicaSample[] }));
    },
  };
}

export type ReplicaSamplesService = ReturnType<typeof createReplicaSamplesService>;
export type { ReplicaSample, ReplicaSampleSeries };
