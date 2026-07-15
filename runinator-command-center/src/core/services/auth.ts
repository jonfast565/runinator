import {
  fetchAuthConfig,
  fetchAuthMe,
  login as apiLogin,
  logout as apiLogout,
  refreshSession,
  setAccessToken,
  type LoginResult,
} from "../api/commandCenterApi";
import { ALL_CAPABILITIES, type Capability, type JsonRecord } from "../domain/models";
import { getPlatformAdapterOptional } from "../platform";
import type { AuthStorage } from "../platform/types";
import { createStore } from "./event-bus";

const ACCESS_KEY = "runinator.auth.access";
const REFRESH_KEY = "runinator.auth.refresh";

export interface AuthState {
  required: boolean;
  authenticated: boolean;
  ready: boolean;
  user: JsonRecord | null;
  // the caller's resolved capability set (see runinator-models capabilities). the whole ui gates
  // against this; auth-disabled stacks ignore it (every capability is granted).
  capabilities: Capability[];
  error: string;
  accessTokenRevision: number;
}

function isCapability(value: unknown): value is Capability {
  return typeof value === "string" && (ALL_CAPABILITIES as readonly string[]).includes(value);
}

function readCapabilities(source: unknown): Capability[] {
  const raw = (source as { capabilities?: unknown } | null)?.capabilities;
  return Array.isArray(raw) ? raw.filter(isCapability) : [];
}

const fallbackAuthStorage: AuthStorage = {
  get(key) {
    try {
      return localStorage.getItem(key);
    } catch {
      return null;
    }
  },
  set(key, value) {
    try {
      localStorage.setItem(key, value);
    } catch {
      /* storage unavailable */
    }
  },
  remove(key) {
    try {
      localStorage.removeItem(key);
    } catch {
      /* storage unavailable */
    }
  },
};

function authStorage(): AuthStorage {
  return getPlatformAdapterOptional()?.authStorage ?? fallbackAuthStorage;
}

function safeGet(key: string): string | null {
  return authStorage().get(key);
}

export function createAuthService() {
  let refreshToken: string | null = null;
  const store = createStore<AuthState>({
    required: false,
    authenticated: false,
    ready: false,
    user: null,
    capabilities: [],
    error: "",
    accessTokenRevision: 0,
  });

  function persist(access: string | null, refresh: string | null) {
    refreshToken = refresh;
    const storage = authStorage();

    if (access) {
      storage.set(ACCESS_KEY, access);
    } else {
      storage.remove(ACCESS_KEY);
    }

    if (refresh) {
      storage.set(REFRESH_KEY, refresh);
    } else {
      storage.remove(REFRESH_KEY);
    }
  }

  async function publishAccessToken(access: string | null) {
    await setAccessToken(access);
    store.setState((state) => ({
      ...state,
      accessTokenRevision: state.accessTokenRevision + 1,
    }));
  }

  async function apply(result: LoginResult) {
    persist(result.access_token, result.refresh_token);
    await publishAccessToken(result.access_token);
    store.setState((state) => ({
      ...state,
      user: result.user,
      capabilities: result.capabilities?.filter(isCapability) ?? [],
      authenticated: true,
    }));
  }

  async function clear() {
    persist(null, null);
    await publishAccessToken(null);
    store.setState((state) => ({
      ...state,
      authenticated: false,
      user: null,
      capabilities: [],
    }));
  }

  async function tryRefresh(token: string): Promise<boolean> {
    try {
      await apply(await refreshSession(token));
      return true;
    } catch {
      await clear();
      return false;
    }
  }

  return {
    ...store,
    resetForTests() {
      refreshToken = null;
      store.setState(() => ({
        required: false,
        authenticated: false,
        ready: false,
        user: null,
        capabilities: [],
        error: "",
        accessTokenRevision: 0,
      }));
    },
    async init() {
      try {
        const config = await fetchAuthConfig();
        store.setState((state) => ({ ...state, required: config.enabled }));
      } catch {
        store.setState((state) => ({ ...state, required: false }));
      }

      const required = store.getState().required;

      if (!required) {
        store.setState((state) => ({ ...state, authenticated: true, ready: true }));
        return;
      }

      const access = safeGet(ACCESS_KEY);
      const refresh = safeGet(REFRESH_KEY);

      if (access) {
        refreshToken = refresh;
        await publishAccessToken(access);

        try {
          const user = await fetchAuthMe();
          store.setState((state) => ({
            ...state,
            user,
            capabilities: readCapabilities(user),
            authenticated: true,
          }));
        } catch {
          const authenticated = refresh ? await tryRefresh(refresh) : false;
          store.setState((state) => ({ ...state, authenticated }));
        }
      }

      store.setState((state) => ({ ...state, ready: true }));
    },
    async signIn(username: string, password: string): Promise<boolean> {
      store.setState((state) => ({ ...state, error: "" }));

      try {
        await apply(await apiLogin(username, password));
        return true;
      } catch (err) {
        store.setState((state) => ({
          ...state,
          error: err instanceof Error ? err.message : String(err),
        }));
        return false;
      }
    },
    async signOut() {
      if (refreshToken) {
        try {
          await apiLogout(refreshToken);
        } catch {
          /* best effort */
        }
      }

      await clear();
    },
    async applyAccessToken(access: string) {
      persist(access, refreshToken);
      await publishAccessToken(access);
    },
    /// re-hydrate the principal (and its capabilities) under the current token. called after an org
    /// switch, where the token — and therefore the org-derived capability set — changes.
    async reloadMe() {
      if (!store.getState().required) {
        return;
      }

      try {
        const user = await fetchAuthMe();
        store.setState((state) => ({ ...state, user, capabilities: readCapabilities(user) }));
      } catch {
        /* keep the current principal on a transient failure */
      }
    },
  };
}

export type AuthService = ReturnType<typeof createAuthService>;
