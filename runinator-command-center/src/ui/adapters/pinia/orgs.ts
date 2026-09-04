import { defineStore } from "pinia";
import { computed } from "vue";
import { orgsService } from "../../../core/services";
import type { RefreshOrgsOptions } from "../../../core/services/orgs";
import { mirrorServiceState } from "./sync";

export type { OrgRole } from "../../../core/api/commandCenterApi";

export const useOrgsStore = defineStore("orgs", () => {
  const state = mirrorServiceState(orgsService);

  return {
    memberships: computed(() => state.value.memberships),
    activeOrgId: computed(() => state.value.activeOrgId),
    activeMembership: computed(() => orgsService.activeMembership()),
    activeOrg: computed(() => orgsService.activeOrg()),
    activeRole: computed(() => orgsService.activeRole()),
    isActiveOrgAdmin: computed(() => orgsService.isActiveOrgAdmin()),
    hasOrgs: computed(() => orgsService.hasOrgs()),
    refresh: (options?: RefreshOrgsOptions) => orgsService.refresh(options),
    setActive: (orgId: string) => orgsService.setActive(orgId),
    setActivePlatform: () => orgsService.setActivePlatform(),
    create: (name: string) => orgsService.create(name),
    clear: () => {
      orgsService.clear();
    },
  };
});
