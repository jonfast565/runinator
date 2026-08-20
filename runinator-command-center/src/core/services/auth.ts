import {
  fetchAuthConfig,
  fetchAuthMe,
  login as apiLogin,
  logout as apiLogout,
  refreshSession,
  setAccessToken,
  type LoginResult,
} from "../api/commandCenterApi";
import { type Action, type JsonRecord } from "../domain/models";
import { getPlatformAdapterOptional } from "../platform";
import type { AuthStorage } from "../platform/types";
import { setUnauthorizedHandler } from "../api/runtime";
import { createStore } from "./event-bus";

const ACCESS_KEY = "runinator.auth.access";
const REFRESH_KEY = "runinator.auth.refresh";

export interface AuthState {
  required: boolean;
  authenticated: boolean;
  ready: boolean;
  user: JsonRecord | null;
  effectiveActions: Action[];
  error: string;
  accessTokenRevision: number;
}

function isAction(value: unknown): value is Action {
  return typeof value === "string";
}

function readEffectiveActions(source: unknown): Action[] {
  const raw = (source as { effective_actions?: unknown } | null)?.effective_actions;
  return Array.isArray(raw) ? raw.filter(isAction) : [];
}

function readPrincipal(source: unknown): JsonRecord | null {
  const record = source as JsonRecord | null;
  if (!record) {return null;}
  const principal = record.principal;
  return principal && typeof principal === "object" ? principal as JsonRecord : record;
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
  let refreshPromise: Promise<boolean> | null = null;
  let refreshTimer: ReturnType<typeof setTimeout> | null = null;
  const store = createStore<AuthState>({
    required: false,
    authenticated: false,
    ready: false,
    user: null,
    effectiveActions: [],
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
      user: readPrincipal(result.user),
      effectiveActions: result.effective_actions.filter(isAction),
      authenticated: true,
    }));
    if (Number.isFinite(result.expires_in) && result.expires_in > 0) {
      if (refreshTimer !== null) clearTimeout(refreshTimer);
      refreshTimer = setTimeout(() => void refreshCurrentSession(), Math.max(5000, result.expires_in * 750));
    }
  }

  function scheduleAccessTokenRefresh(access: string) {
    try {
      const payload = access.split(".")[1];
      const normalized = payload.replace(/-/g, "+").replace(/_/g, "/");
      const padded = normalized + "=".repeat((4 - (normalized.length % 4)) % 4);
      const decoded = JSON.parse(atob(padded)) as { exp?: number };
      if (typeof decoded.exp === "number") {
        const delay = Math.max(5000, Math.floor((decoded.exp * 1000 - Date.now()) * 0.75));
        if (refreshTimer !== null) clearTimeout(refreshTimer);
        refreshTimer = setTimeout(() => void refreshCurrentSession(), delay);
      }
    } catch {
      // Opaque/API-key credentials do not carry a client-readable expiry; 401 recovery remains the
      // fallback for those credentials.
    }
  }

  async function clear() {
    if (refreshTimer !== null) {
      clearTimeout(refreshTimer);
      refreshTimer = null;
    }
    persist(null, null);
    await publishAccessToken(null);
    store.setState((state) => ({
      ...state,
      authenticated: false,
      user: null,
      effectiveActions: [],
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

  async function refreshCurrentSession(): Promise<boolean> {
    if (refreshPromise) return refreshPromise;
    const token = refreshToken;
    if (!token || !store.getState().required) return false;
    refreshPromise = tryRefresh(token).finally(() => { refreshPromise = null; });
    return refreshPromise;
  }

  setUnauthorizedHandler(async () => {
    if (!store.getState().authenticated) return false;
    return refreshCurrentSession();
  });

  return {
    ...store,
    resetForTests() {
      if (refreshTimer !== null) clearTimeout(refreshTimer);
      refreshTimer = null;
      setUnauthorizedHandler(null);
      refreshToken = null;
      store.setState(() => ({
        required: false,
        authenticated: false,
        ready: false,
        user: null,
        effectiveActions: [],
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
        scheduleAccessTokenRefresh(access);

        try {
          const user = await fetchAuthMe();
          store.setState((state) => ({
            ...state,
            user: readPrincipal(user),
            effectiveActions: readEffectiveActions(user),
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
    refresh: refreshCurrentSession,
    async applyAccessToken(access: string) {
      persist(access, refreshToken);
      await publishAccessToken(access);
    },
    // re-hydrate the principal (and its actions) under the current token. called after an org
    // switch, where the token — and therefore the org-derived action set — changes.
    async reloadMe() {
      if (!store.getState().required) {
        return;
      }

      try {
        const user = await fetchAuthMe();
        store.setState((state) => ({ ...state, user: readPrincipal(user), effectiveActions: readEffectiveActions(user) }));
      } catch {
        /* keep the current principal on a transient failure */
      }
    },
  };
}

export type AuthService = ReturnType<typeof createAuthService>;
