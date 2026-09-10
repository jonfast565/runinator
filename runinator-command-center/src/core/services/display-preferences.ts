import { createStore } from "./event-bus";

export type AppTheme = "system" | "light" | "dark";

const THEME_KEY = "command-center.theme";
const DEFAULT_TAB_KEY = "command-center.defaultTab";
const HIDDEN_TIMELINE_EVENT_CATEGORIES_KEY = "command-center.timeline.hiddenCategories";
const SHOW_SYSTEM_TIMELINE_EVENTS_KEY = "command-center.timeline.showSystemEvents";

export const DEFAULT_TAB_OPTIONS = [
  { value: "Workflows", label: "Workflows" },
  { value: "Runs", label: "Workflow Runs" },
  { value: "Providers", label: "Providers" },
  { value: "Replicas", label: "Replicas" },
  { value: "Approvals", label: "Approvals" },
  { value: "Notifications", label: "Notifications" },
] as const;

const ALLOWED_THEMES: AppTheme[] = ["system", "light", "dark"];
const ALLOWED_TABS = DEFAULT_TAB_OPTIONS.map((option) => option.value);

export interface DisplayPreferencesState {
  theme: AppTheme;
  defaultTab: string;
  hiddenTimelineEventCategories: string[];
}

function readStored<T extends string>(key: string, allowed: T[], fallback: T): T {
  try {
    const stored = localStorage.getItem(key);

    if (stored && (allowed as string[]).includes(stored)) {
      return stored as T;
    }
  } catch {
    // storage unavailable; use fallback.
  }

  return fallback;
}

function writeStored(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    // storage unavailable; preference is memory-only.
  }
}

function readStoredBoolean(key: string, fallback: boolean): boolean {
  try {
    const stored = localStorage.getItem(key);

    if (stored === "true" || stored === "false") {
      return stored === "true";
    }
  } catch {
    // storage unavailable; use fallback.
  }

  return fallback;
}

function readHiddenTimelineEventCategories(): string[] {
  try {
    const stored = localStorage.getItem(HIDDEN_TIMELINE_EVENT_CATEGORIES_KEY);
    const parsed: unknown = stored ? JSON.parse(stored) : null;

    if (Array.isArray(parsed)) {
      return [
        ...new Set(
          parsed
            .filter((value): value is string => typeof value === "string")
            .map((value) => value.trim().toLowerCase())
            .filter(Boolean),
        ),
      ];
    }
  } catch {
    // invalid or unavailable storage falls through to the legacy preference.
  }

  return readStoredBoolean(SHOW_SYSTEM_TIMELINE_EVENTS_KEY, true) ? [] : ["system"];
}

export function createDisplayPreferencesService() {
  const store = createStore<DisplayPreferencesState>({
    theme: readStored(THEME_KEY, ALLOWED_THEMES, "system"),
    defaultTab: readStored(DEFAULT_TAB_KEY, ALLOWED_TABS as unknown as string[], "Workflows"),
    hiddenTimelineEventCategories: readHiddenTimelineEventCategories(),
  });

  const service = {
    ...store,
    setTheme(theme: AppTheme) {
      store.setState((state) => ({ ...state, theme }));
      writeStored(THEME_KEY, theme);
    },
    setDefaultTab(defaultTab: string) {
      store.setState((state) => ({ ...state, defaultTab }));
      writeStored(DEFAULT_TAB_KEY, defaultTab);
    },
    setTimelineEventCategoryVisible(categoryId: string, visible: boolean) {
      const normalized = categoryId.trim().toLowerCase();

      if (!normalized) {
        return;
      }

      store.setState((state) => {
        const hidden = new Set(state.hiddenTimelineEventCategories);

        if (visible) {
          hidden.delete(normalized);
        } else {
          hidden.add(normalized);
        }

        const hiddenTimelineEventCategories = [...hidden].sort();
        writeStored(
          HIDDEN_TIMELINE_EVENT_CATEGORIES_KEY,
          JSON.stringify(hiddenTimelineEventCategories),
        );
        return { ...state, hiddenTimelineEventCategories };
      });
    },
  };

  return service;
}

export type DisplayPreferencesService = ReturnType<typeof createDisplayPreferencesService>;
