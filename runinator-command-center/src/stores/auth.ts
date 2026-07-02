import { defineStore } from "pinia";
import { ref } from "vue";
import {
  fetchAuthConfig,
  fetchAuthMe,
  login as apiLogin,
  logout as apiLogout,
  refreshSession,
  setAccessToken,
  type LoginResult,
} from "../api/commandCenterApi";
import type { JsonRecord } from "../types/models";

const ACCESS_KEY = "runinator.auth.access";
const REFRESH_KEY = "runinator.auth.refresh";

function safeGet(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

// holds session state for the command center; the access token rides in the http/ws layers while
// the refresh token persists locally so logins survive reloads.
export const useAuthStore = defineStore("auth", () => {
  const required = ref(false);
  const authenticated = ref(false);
  const ready = ref(false);
  const user = ref<JsonRecord | null>(null);
  const error = ref("");
  const accessTokenRevision = ref(0);
  let refreshToken: string | null = null;

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
      /* storage unavailable; session is then memory-only */
    }
  }

  async function apply(result: LoginResult) {
    persist(result.access_token, result.refresh_token);
    await publishAccessToken(result.access_token);
    user.value = result.user;
    authenticated.value = true;
  }

  async function clear() {
    persist(null, null);
    await publishAccessToken(null);
    authenticated.value = false;
    user.value = null;
  }

  // swap the active access token in place (e.g. after switching org) while keeping the refresh token.
  async function applyAccessToken(access: string) {
    persist(access, refreshToken);
    await publishAccessToken(access);
  }

  async function publishAccessToken(access: string | null) {
    await setAccessToken(access);
    accessTokenRevision.value += 1;
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

  // probe whether auth is required, then restore any persisted session.
  async function init() {
    try {
      required.value = (await fetchAuthConfig()).enabled;
    } catch {
      required.value = false;
    }

    if (!required.value) {
      authenticated.value = true;
      ready.value = true;
      return;
    }

    const access = safeGet(ACCESS_KEY);
    const refresh = safeGet(REFRESH_KEY);

    if (access) {
      refreshToken = refresh;
      await publishAccessToken(access);

      try {
        user.value = await fetchAuthMe();
        authenticated.value = true;
      } catch {
        authenticated.value = refresh ? await tryRefresh(refresh) : false;
      }
    }

    ready.value = true;
  }

  async function signIn(username: string, password: string): Promise<boolean> {
    error.value = "";

    try {
      await apply(await apiLogin(username, password));
      return true;
    } catch (err) {
      error.value = err instanceof Error ? err.message : String(err);
      return false;
    }
  }

  async function signOut() {
    if (refreshToken) {
      try {
        await apiLogout(refreshToken);
      } catch {
        /* best effort */
      }
    }

    await clear();
  }

  return {
    required,
    authenticated,
    ready,
    user,
    error,
    accessTokenRevision,
    init,
    signIn,
    signOut,
    applyAccessToken,
  };
});
