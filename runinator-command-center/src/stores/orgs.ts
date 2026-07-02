import { defineStore } from "pinia";
import { computed, ref } from "vue";
import {
  createOrg as apiCreateOrg,
  listMyOrgs,
  switchOrg,
  type OrgMembershipView,
  type OrgRole,
} from "../api/commandCenterApi";
import { useAppStore } from "./app";
import { useAuthStore } from "./auth";

const ACTIVE_ORG_KEY = "runinator.org.active";

function safeGet(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function safeSet(key: string, value: string | null) {
  try {
    if (value) {
      localStorage.setItem(key, value);
    } else {
      localStorage.removeItem(key);
    }
  } catch {
    /* storage unavailable; active org is then memory-only */
  }
}

const ROLE_RANK: Record<OrgRole, number> = { member: 0, admin: 1, owner: 2 };

// holds the caller's org memberships and the active org. switching re-issues the access token with
// the new org claim so every subsequent request is scoped to that tenant.
export const useOrgsStore = defineStore("orgs", () => {
  const app = useAppStore();
  const auth = useAuthStore();

  const memberships = ref<OrgMembershipView[]>([]);
  const activeOrgId = ref<string | null>(safeGet(ACTIVE_ORG_KEY));

  const activeMembership = computed(
    () => memberships.value.find((m) => m.org.id === activeOrgId.value) ?? null,
  );
  const activeOrg = computed(() => activeMembership.value?.org ?? null);
  const activeRole = computed<OrgRole | null>(() => activeMembership.value?.role ?? null);
  const isActiveOrgAdmin = computed(
    () => activeRole.value != null && ROLE_RANK[activeRole.value] >= ROLE_RANK.admin,
  );
  const hasOrgs = computed(() => memberships.value.length > 0);

  async function refresh() {
    memberships.value = await app
      .runOperation("Loading organizations", () => listMyOrgs())
      .catch(() => []);

    // drop a stale active selection; auto-select the first org when none is active.
    if (activeOrgId.value && !memberships.value.some((m) => m.org.id === activeOrgId.value)) {
      setActiveLocal(null);
    }

    if (!activeOrgId.value && memberships.value.length > 0) {
      await setActive(memberships.value[0].org.id);
    }
  }

  function setActiveLocal(orgId: string | null) {
    activeOrgId.value = orgId;
    safeSet(ACTIVE_ORG_KEY, orgId);
  }

  // switch the active org: re-issue the access token bound to it, then remember the selection.
  async function setActive(orgId: string): Promise<boolean> {
    try {
      const context = await switchOrg(orgId);
      await auth.applyAccessToken(context.access_token);
      setActiveLocal(orgId);
      app.setStatus(`Active organization: ${context.org.name}`);
      return true;
    } catch (err) {
      app.setError(err instanceof Error ? err.message : String(err));
      return false;
    }
  }

  async function create(name: string): Promise<boolean> {
    const org = await app
      .runOperation("Creating organization", () => apiCreateOrg(name))
      .catch(() => null);

    if (!org) {
      return false;
    }

    await refresh();
    await setActive(org.id);
    return true;
  }

  function clear() {
    memberships.value = [];
    setActiveLocal(null);
  }

  return {
    memberships,
    activeOrgId,
    activeOrg,
    activeRole,
    activeMembership,
    isActiveOrgAdmin,
    hasOrgs,
    refresh,
    setActive,
    create,
    clear,
  };
});
