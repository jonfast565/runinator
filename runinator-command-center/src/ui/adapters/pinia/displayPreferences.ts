import { defineStore } from "pinia";
import { computed } from "vue";
import {
  DEFAULT_TAB_OPTIONS,
  type AppTheme,
  type DisplayPreferencesState,
} from "../../../core/services/display-preferences";
import { displayPreferencesService } from "../../../core/services";
import { mirrorServiceState } from "./sync";

export type { AppTheme };
export { DEFAULT_TAB_OPTIONS };

export const useDisplayPreferencesStore = defineStore("displayPreferences", () => {
  const state = mirrorServiceState<DisplayPreferencesState>(displayPreferencesService);

  return {
    theme: computed({
      get: () => state.value.theme,
      set: (theme: AppTheme) => {
        displayPreferencesService.setTheme(theme);
      },
    }),
    defaultTab: computed({
      get: () => state.value.defaultTab,
      set: (defaultTab: string) => {
        displayPreferencesService.setDefaultTab(defaultTab);
      },
    }),
    showSystemTimelineEvents: computed({
      get: () => state.value.showSystemTimelineEvents,
      set: (showSystemTimelineEvents: boolean) => {
        displayPreferencesService.setShowSystemTimelineEvents(showSystemTimelineEvents);
      },
    }),
    setTheme: (theme: AppTheme) => {
      displayPreferencesService.setTheme(theme);
    },
    setDefaultTab: (defaultTab: string) => {
      displayPreferencesService.setDefaultTab(defaultTab);
    },
    setShowSystemTimelineEvents: (showSystemTimelineEvents: boolean) => {
      displayPreferencesService.setShowSystemTimelineEvents(showSystemTimelineEvents);
    },
  };
});
