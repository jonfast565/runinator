import {
  deleteExecutionProfile,
  fetchExecutionProfiles,
  putExecutionProfile,
  rotateExecutionProfile,
  testExecutionProfile,
} from "../api/commandCenterApi";
import type { ExecutionProfile, ExecutionProfileInput } from "../domain/models";
import type { AppService } from "./app";
import { createStore } from "./event-bus";

export interface ExecutionProfilesState {
  profiles: ExecutionProfile[];
}

export function createExecutionProfilesService(app: AppService) {
  const store = createStore<ExecutionProfilesState>({ profiles: [] });
  const service = {
    ...store,
    async refresh() {
      const profiles = await app.runOperation(
        "Refreshing execution profiles",
        () => fetchExecutionProfiles(),
        { retryable: true },
      );
      store.setState(() => ({ profiles }));
    },
    clear() {
      store.setState(() => ({ profiles: [] }));
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
      await app.runOperation("Testing execution profile", () => testExecutionProfile(id));
      await service.refresh();
    },
  };
  return service;
}

export type ExecutionProfilesService = ReturnType<typeof createExecutionProfilesService>;
