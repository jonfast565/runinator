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
    runtimeCatalog: computed(() => state.value.runtimeCatalog),
    refresh: () => adminSettingsService.refresh(),
    refreshServerSettings: () => adminSettingsService.refreshServerSettings(),
    refreshAuthSettings: () => adminSettingsService.refreshAuthSettings(),
    saveLanguage: (language: string) => adminSettingsService.saveLanguage(language),
    saveAuthSettings: () => adminSettingsService.saveAuthSettings(),
    saveServerSettings: () => adminSettingsService.saveServerSettings(),
    updateServerSetting: (key: string, value: number | boolean) => {
      adminSettingsService.updateServerSetting(key, value);
    },
    updateMaxRefreshes: (value: number) => { adminSettingsService.updateMaxRefreshes(value); },
    clear: () => { adminSettingsService.clear(); },
    updateLanguageField: (
      language: string,
      field: "image" | "setup_script" | "environment_text" | "executable" | "build_args_text" | "run_args_text",
      value: string,
    ) => { adminSettingsService.updateLanguageField(language, field, value); },
    updateLanguageLimit: (
      language: string,
      field: "memory_mb" | "cpu_millis" | "pids" | "tmpfs_mb" | "max_output_bytes",
      value: number,
    ) => { adminSettingsService.updateLanguageLimit(language, field, value); },
  };
});
