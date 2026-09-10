import { defaultApi, type ExecutionProfilesApi } from "../api/ports/execution-profiles";

import type {
  ExecutionProfile,
  ExecutionProfileCollectionStatus,
  ExecutionProfileInput,
} from "../domain/models";
import type { AppService } from "./app";
import { createStore } from "./event-bus";

export interface ExecutionProfilesState {
  profiles: ExecutionProfile[];
  collectionStatuses: Record<string, ExecutionProfileCollectionStatus>;
}

export function createExecutionProfilesService(
  app: AppService,
  api: ExecutionProfilesApi = defaultApi,
) {
  const store = createStore<ExecutionProfilesState>({ profiles: [], collectionStatuses: {} });
  let refreshTimer: ReturnType<typeof setTimeout> | undefined;
  let backgroundRefresh: Promise<void> | undefined;
  let refreshRevision = 0;
  let generation = 0;
  const indexStatuses = (statuses: ExecutionProfileCollectionStatus[]) =>
    Object.fromEntries(statuses.map((status) => [status.profile_id, status]));
  const service = {
    ...store,
    async refresh() {
      const [profiles, statuses] = await app.runOperation(
        "Refreshing execution profiles",
        () =>
          Promise.all([
            api.fetchExecutionProfiles(),
            api.fetchExecutionProfileCollectionStatuses(),
          ]),
        { retryable: true },
      );
      store.setState(() => ({ profiles, collectionStatuses: indexStatuses(statuses) }));
    },
    scheduleCollectionStatusRefresh() {
      if (refreshTimer !== undefined) {
        return;
      }

      refreshTimer = setTimeout(() => {
        refreshTimer = undefined;
        void service.refreshCollectionStatus().catch(() => undefined);
      }, 100);
    },
    refreshCollectionStatus(): Promise<void> {
      refreshRevision += 1;

      if (backgroundRefresh) {
        return backgroundRefresh;
      }

      const startedGeneration = generation;
      backgroundRefresh = (async () => {
        let fetchedRevision = -1;

        while (fetchedRevision < refreshRevision) {
          fetchedRevision = refreshRevision;
          const [profiles, statuses] = await Promise.all([
            api.fetchExecutionProfiles(),
            api.fetchExecutionProfileCollectionStatuses(),
          ]);

          if (startedGeneration !== generation) {
            return;
          }

          store.setState(() => ({ profiles, collectionStatuses: indexStatuses(statuses) }));
        }
      })().finally(() => {
        backgroundRefresh = undefined;
      });
      return backgroundRefresh;
    },
    clear() {
      generation += 1;
      clearTimeout(refreshTimer);
      refreshTimer = undefined;
      store.setState(() => ({ profiles: [], collectionStatuses: {} }));
    },
    async save(id: string, profile: ExecutionProfileInput) {
      await app.runOperation("Saving execution profile", () =>
        api.putExecutionProfile(id, profile),
      );
      await service.refresh();
    },
    async remove(id: string) {
      await app.runOperation("Deleting execution profile", () => api.deleteExecutionProfile(id));
      await service.refresh();
    },
    async rotate(id: string) {
      await app.runOperation("Rotating execution profile", () => api.rotateExecutionProfile(id));
      await service.refresh();
    },
    async test(id: string) {
      await app.runOperation("Dry-running execution profile collection", () =>
        api.testExecutionProfile(id),
      );
      await service.refresh();
    },
  };
  return service;
}

export type ExecutionProfilesService = ReturnType<typeof createExecutionProfilesService>;
