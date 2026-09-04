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
    async refreshCollectionStatus() {
      const statuses = await fetchExecutionProfileCollectionStatuses();
      store.setState((state) => ({
        ...state,
        collectionStatuses: indexStatuses(statuses),
      }));
    },
    clear() {
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
