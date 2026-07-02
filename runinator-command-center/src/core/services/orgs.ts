import {
  createOrg as apiCreateOrg,
  listMyOrgs,
  switchOrg,
  type OrgMembershipView,
  type OrgRole,
} from "../api/commandCenterApi";
import { createStore } from "./event-bus";
import type { AppService } from "./app";
import type { AuthService } from "./auth";

const ACTIVE_ORG_KEY = "runinator.org.active";

export const ORG_ROLE_RANK: Record<OrgRole, number> = { member: 0, admin: 1, owner: 2 };

export interface OrgsState {
  memberships: OrgMembershipView[];
  activeOrgId: string | null;
}

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
    // storage unavailable; active org is then memory-only.
  }
}

export function createOrgsService(app: AppService, auth: AuthService) {
  const store = createStore<OrgsState>({
    memberships: [],
    activeOrgId: safeGet(ACTIVE_ORG_KEY),
  });

  function activeMembership(): OrgMembershipView | null {
    const { memberships, activeOrgId } = store.getState();
    return memberships.find((membership) => membership.org.id === activeOrgId) ?? null;
  }

  function activeOrg() {
    return activeMembership()?.org ?? null;
  }

  function activeRole(): OrgRole | null {
    return activeMembership()?.role ?? null;
  }

  function isActiveOrgAdmin(): boolean {
    const role = activeRole();
    return role != null && ORG_ROLE_RANK[role] >= ORG_ROLE_RANK.admin;
  }

  function hasOrgs(): boolean {
    return store.getState().memberships.length > 0;
  }

  const service = {
    ...store,
    activeMembership,
    activeOrg,
    activeRole,
    isActiveOrgAdmin,
    hasOrgs,
    setActiveLocal(orgId: string | null) {
      store.setState((state) => ({ ...state, activeOrgId: orgId }));
      safeSet(ACTIVE_ORG_KEY, orgId);
    },
    async refresh() {
      const memberships = await app
        .runOperation("Loading organizations", () => listMyOrgs())
        .catch(() => []);

      let activeOrgId = store.getState().activeOrgId;

      if (activeOrgId && !memberships.some((membership) => membership.org.id === activeOrgId)) {
        service.setActiveLocal(null);
        activeOrgId = null;
      }

      store.setState((state) => ({ ...state, memberships }));

      if (!activeOrgId && memberships.length > 0) {
        await service.setActive(memberships[0].org.id);
      }
    },
    async setActive(orgId: string): Promise<boolean> {
      try {
        const context = await switchOrg(orgId);
        await auth.applyAccessToken(context.access_token);
        service.setActiveLocal(orgId);
        app.setStatus(`Active organization: ${context.org.name}`);
        return true;
      } catch (err) {
        app.setError(err instanceof Error ? err.message : String(err));
        return false;
      }
    },
    async create(name: string): Promise<boolean> {
      const org = await app
        .runOperation("Creating organization", () => apiCreateOrg(name))
        .catch(() => null);

      if (!org) {
        return false;
      }

      await service.refresh();
      await service.setActive(org.id);
      return true;
    },
    clear() {
      store.setState((state) => ({ ...state, memberships: [] }));
      service.setActiveLocal(null);
    },
  };

  return service;
}

export type OrgsService = ReturnType<typeof createOrgsService>;
