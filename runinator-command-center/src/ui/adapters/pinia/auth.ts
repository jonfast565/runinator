import { defineStore } from "pinia";
import { computed } from "vue";
import { authService } from "../../../core/services";
import { mirrorServiceState } from "./sync";

export const useAuthStore = defineStore("auth", () => {
  const state = mirrorServiceState(authService);

  return {
    required: computed(() => state.value.required),
    authenticated: computed(() => state.value.authenticated),
    ready: computed(() => state.value.ready),
    user: computed(() => state.value.user),
    error: computed(() => state.value.error),
    accessTokenRevision: computed(() => state.value.accessTokenRevision),
    init: () => authService.init(),
    signIn: (username: string, password: string) => authService.signIn(username, password),
    signOut: () => authService.signOut(),
    applyAccessToken: (access: string) => authService.applyAccessToken(access),
  };
});
