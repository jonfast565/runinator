import {
  fetchAuthConfig,
  fetchAuthMe,
  login as apiLogin,
  logout as apiLogout,
  refreshSession,
  setAccessToken,
  type LoginResult,
} from "../api/commandCenterApi";
import type { JsonRecord } from "../domain/models";
import { createStore } from "./event-bus";

const ACCESS_KEY = "runinator.auth.access";
const REFRESH_KEY = "runinator.auth.refresh";

export interface AuthState {
  required: boolean;
  authenticated: boolean;
  ready: boolean;
  user: JsonRecord | null;
  error: string;
  accessTokenRevision: number;
}

function safeGet(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

export function createAuthService() {
  let refreshToken: string | null = null;
  const store = createStore<AuthState>({
    required: false,
    authenticated: false,
    ready: false,
    user: null,
    error: "",
    accessTokenRevision: 0,
  });

  function persist(access: string | null, refresh: string | null) {
    refreshToken = refresh;

    try {
      if (access) {
        localStorage.setItem(ACCESS_KEY, access);
      } else {
        localStorage.removeItem(ACCESS_KEY);
      }

      if (refresh) {
        localStorage.setItem(REFRESH_KEY, refresh);
      } else {
        localStorage.removeItem(REFRESH_KEY);
      }
    } catch {
      /* storage unavailable */
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
          store.setState((state) => ({ ...state, user, authenticated: true }));
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
  };
}

export type AuthService = ReturnType<typeof createAuthService>;
