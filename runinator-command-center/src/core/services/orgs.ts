import {
  createOrg as apiCreateOrg,
  listMyOrgs,
  switchOrg,
  switchPlatform,
  updateOrg as apiUpdateOrg,
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

export interface RefreshOrgsOptions {
  /** Select the first organization returned by the server, ignoring a remembered choice. */
  selectDefault?: boolean;
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
    async refresh({ selectDefault = false }: RefreshOrgsOptions = {}) {
      const memberships = await app
        .runOperation("Loading organizations", () => listMyOrgs())
        .catch(() => []);

      let activeOrgId = selectDefault ? null : store.getState().activeOrgId;

      if (selectDefault) {
        // A new sign-in starts in the server's stable default order, not a previous user's
        // browser-local selection. The following switch mints a token with that org's scope.
        service.setActiveLocal(null);
      }

      if (activeOrgId && !memberships.some((membership) => membership.org.id === activeOrgId)) {
        service.setActiveLocal(null);
        activeOrgId = null;
      }

      store.setState((state) => ({ ...state, memberships }));

      const isPlatformAdmin = auth.getState().user?.platform_role === "admin";

      if (isPlatformAdmin && !activeOrgId) {
        return;
      }

      if (selectDefault && memberships.length > 0) {
        await service.setActive(memberships[0].org.id);
      } else if (!activeOrgId && memberships.length > 0) {
        await service.setActive(memberships[0].org.id);
      }
    },
    async setActive(orgId: string): Promise<boolean> {
      try {
        const context = await switchOrg(orgId);
        await auth.applyAccessToken(context.access_token);
        // the new token carries the org role, so refresh the principal to pick up org actions.
        await auth.reloadMe();
        service.setActiveLocal(orgId);
        app.setStatus(`Active organization: ${context.org.name}`);
        return true;
      } catch (err) {
        app.setError(err instanceof Error ? err.message : String(err));
        return false;
      }
    },
    async setActivePlatform(): Promise<boolean> {
      if (!auth.getState().required) {
        service.setActiveLocal(null);
        app.setStatus("Active scope: Platform");
        return true;
      }

      try {
        const context = await switchPlatform();
        await auth.applyAccessToken(context.access_token);
        await auth.reloadMe();
      } catch {
        app.setError("Could not switch to platform scope");
        return false;
      }

      service.setActiveLocal(null);
      app.setStatus("Active scope: Platform");
      return true;
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
    async rename(name: string): Promise<boolean> {
      const orgId = store.getState().activeOrgId;

      if (!orgId) {
        return false;
      }

      const org = await app
        .runOperation("Renaming organization", () => apiUpdateOrg(orgId, name))
        .catch(() => null);

      if (!org) {
        return false;
      }

      store.setState((state) => ({
        ...state,
        memberships: state.memberships.map((membership) =>
          membership.org.id === orgId ? { ...membership, org } : membership,
        ),
      }));
      app.setStatus(`Organization renamed to: ${org.name}`);
      return true;
    },
    clear() {
      store.setState((state) => ({ ...state, memberships: [] }));
      service.setActiveLocal(null);
    },
  };

  auth.registerScopeRestorer(async () => {
    const orgId = store.getState().activeOrgId;

    if (!orgId) {
      return;
    }

    if (!(await service.setActive(orgId))) {
      // The user may have been removed from the organization while their token was refreshed.
      // Keep the valid platform token rather than displaying an organization it no longer covers.
      service.setActiveLocal(null);
    }
  });

  return service;
}

export type OrgsService = ReturnType<typeof createOrgsService>;
