import { defineStore } from "pinia";
import { computed } from "vue";
import { appService, executionProfilesService } from "../../../core/services";
import type { ExecutionProfileInput } from "../../../core/domain/models";
import { mirrorServiceState } from "./sync";

export const useExecutionProfilesStore = defineStore("execution-profiles", () => {
  const state = mirrorServiceState(executionProfilesService);
  return {
    profiles: computed(() => state.value.profiles),
    collectionStatuses: computed(() => state.value.collectionStatuses),
    filteredProfiles: computed(() => {
      const query = appService.normalizedSearch;
      return query
        ? state.value.profiles.filter((profile) =>
            [profile.name, profile.description, ...profile.credential_scopes].some((value) =>
              value.toLowerCase().includes(query),
            ),
          )
        : state.value.profiles;
    }),
    refresh: () => executionProfilesService.refresh(),
    refreshCollectionStatus: () => executionProfilesService.refreshCollectionStatus(),
    clear: () => {
      executionProfilesService.clear();
    },
    save: (id: string, profile: ExecutionProfileInput) =>
      executionProfilesService.save(id, profile),
    remove: (id: string) => executionProfilesService.remove(id),
    rotate: (id: string) => executionProfilesService.rotate(id),
    test: (id: string) => executionProfilesService.test(id),
  };
});
