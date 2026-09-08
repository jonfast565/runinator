import {
  deleteExecutionProfile,
  fetchExecutionProfileCollectionStatuses,
  fetchExecutionProfiles,
  putExecutionProfile,
  rotateExecutionProfile,
  testExecutionProfile,
} from "../api/commandCenterApi";
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

export function createExecutionProfilesService(app: AppService) {
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
        () => Promise.all([fetchExecutionProfiles(), fetchExecutionProfileCollectionStatuses()]),
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
            fetchExecutionProfiles(),
            fetchExecutionProfileCollectionStatuses(),
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
      await app.runOperation("Saving execution profile", () => putExecutionProfile(id, profile));
      await service.refresh();
    },
    async remove(id: string) {
      await app.runOperation("Deleting execution profile", () => deleteExecutionProfile(id));
      await service.refresh();
    },
    async rotate(id: string) {
      await app.runOperation("Rotating execution profile", () => rotateExecutionProfile(id));
      await service.refresh();
    },
    async test(id: string) {
      await app.runOperation("Dry-running execution profile collection", () =>
        testExecutionProfile(id),
      );
      await service.refresh();
    },
  };
  return service;
}

export type ExecutionProfilesService = ReturnType<typeof createExecutionProfilesService>;
