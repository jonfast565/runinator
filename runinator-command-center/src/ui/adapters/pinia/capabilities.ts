import { defineStore } from "pinia";
import { computed } from "vue";
import type { Capability } from "../../../core/domain/models";
import { authService } from "../../../core/services";
import { mirrorServiceState } from "./sync";

// capabilities are a property of the authenticated principal (see the auth service), so this store
// mirrors auth state. gating of per-resource actions (view/run/edit/own on a workflow/pipeline) stays
// with the grant-aware services; this covers the platform/org capability axis the whole ui gates on.
export const useCapabilitiesStore = defineStore("capabilities", () => {
  const auth = mirrorServiceState(authService);

  // reads reactive state so callers used in templates re-evaluate when auth or capabilities change.
  function has(capability: Capability): boolean {
    if (!auth.value.required) {
      return true;
    }

    return auth.value.capabilities.includes(capability);
  }

  return {
    capabilities: computed(() => auth.value.capabilities),
    has,
  };
});
