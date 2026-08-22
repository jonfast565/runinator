import { defineStore } from "pinia";
import { computed } from "vue";
import type { Action } from "../../../core/domain/models";
import { authService } from "../../../core/services";
import { mirrorServiceState } from "./sync";

// actions are a property of the authenticated principal (see the auth service), so this store
// mirrors auth state. gating of per-resource actions (view/run/edit/own on a workflow/pipeline) stays
// with grant-aware services. This covers platform and organization actions gated by the UI.
export const useActionsStore = defineStore("actions", () => {
  const auth = mirrorServiceState(authService);

  // reads reactive state so callers used in templates re-evaluate when auth or actions change.
  function has(action: Action): boolean {
    if (!auth.value.required) {
      return true;
    }

    return auth.value.effectiveActions.includes(action);
  }

  return {
    actions: computed(() => auth.value.effectiveActions),
    has,
  };
});
