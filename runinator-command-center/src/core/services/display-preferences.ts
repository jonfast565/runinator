import { createStore } from "./event-bus";

export type AppTheme = "system" | "light" | "dark";

const THEME_KEY = "command-center.theme";
const DEFAULT_TAB_KEY = "command-center.defaultTab";

export const DEFAULT_TAB_OPTIONS = [
  { value: "Workflows", label: "Workflows" },
  { value: "Runs", label: "Runs" },
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

export function createDisplayPreferencesService() {
  const store = createStore<DisplayPreferencesState>({
    theme: readStored(THEME_KEY, ALLOWED_THEMES, "system"),
    defaultTab: readStored(DEFAULT_TAB_KEY, ALLOWED_TABS as unknown as string[], "Workflows"),
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
  };

  return service;
}

export type DisplayPreferencesService = ReturnType<typeof createDisplayPreferencesService>;
