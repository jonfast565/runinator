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
    maxRefreshes: computed(() => state.value.maxRefreshes),
    serverValues: computed(() => state.value.serverValues),
    serverCatalog: computed(() => state.value.serverCatalog),
    refresh: () => adminSettingsService.refresh(),
    refreshServerSettings: () => adminSettingsService.refreshServerSettings(),
    refreshAuthSettings: () => adminSettingsService.refreshAuthSettings(),
    saveLanguage: (language: string) => adminSettingsService.saveLanguage(language),
    saveAuthSettings: () => adminSettingsService.saveAuthSettings(),
    saveServerSettings: () => adminSettingsService.saveServerSettings(),
    updateServerSetting: (key: string, value: number) => {
      adminSettingsService.updateServerSetting(key, value);
    },
    updateMaxRefreshes: (value: number) => { adminSettingsService.updateMaxRefreshes(value); },
    clear: () => { adminSettingsService.clear(); },
    updateLanguageField: (
      language: string,
      field: "image" | "setup_script",
      value: string,
    ) => { adminSettingsService.updateLanguageField(language, field, value); },
  };
});
