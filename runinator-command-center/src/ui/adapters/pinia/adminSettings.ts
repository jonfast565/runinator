import { defineStore } from "pinia";
import { computed } from "vue";
import { adminSettingsService } from "../../../core/services";
import { mirrorServiceState } from "./sync";

export type { ForeignLanguageSetting } from "../../../core/services/admin-settings";

export const useAdminSettingsStore = defineStore("adminSettings", () => {
  const state = mirrorServiceState(adminSettingsService);

  return {
    loaded: computed(() => state.value.loaded),
    languages: computed(() => state.value.languages),
    refresh: () => adminSettingsService.refresh(),
    saveLanguage: (language: string) => adminSettingsService.saveLanguage(language),
    clear: () => { adminSettingsService.clear(); },
    updateLanguageField: (
      language: string,
      field: "image" | "setup_script",
      value: string,
    ) => adminSettingsService.updateLanguageField(language, field, value),
  };
});
