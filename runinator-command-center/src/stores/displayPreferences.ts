import { defineStore } from "pinia";
import { ref, watch } from "vue";

export type AppTheme = "system" | "light" | "dark";

const THEME_KEY = "command-center.theme";
const DEFAULT_TAB_KEY = "command-center.defaultTab";

function readStored<T extends string>(key: string, allowed: T[], fallback: T): T {
  try {
    const stored = localStorage.getItem(key);
    if (stored && (allowed as string[]).includes(stored)) return stored as T;
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

// module-level cleanup for the system-theme OS media listener.
let mediaCleanup: (() => void) | null = null;

function applyTheme(theme: AppTheme) {
  if (mediaCleanup) {
    mediaCleanup();
    mediaCleanup = null;
  }
  if (theme === "system") {
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const update = () => document.documentElement.setAttribute("data-theme", media.matches ? "dark" : "light");
    update();
    media.addEventListener("change", update);
    mediaCleanup = () => media.removeEventListener("change", update);
  } else {
    document.documentElement.setAttribute("data-theme", theme);
  }
}

export const DEFAULT_TAB_OPTIONS = [
  { value: "Workflows", label: "Workflows" },
  { value: "Runs", label: "Runs" },
  { value: "Providers", label: "Providers" },
  { value: "Replicas", label: "Replicas" },
  { value: "Approvals", label: "Approvals" },
  { value: "Notifications", label: "Notifications" }
] as const;

const ALLOWED_THEMES: AppTheme[] = ["system", "light", "dark"];
const ALLOWED_TABS = DEFAULT_TAB_OPTIONS.map((opt) => opt.value);

export const useDisplayPreferencesStore = defineStore("displayPreferences", () => {
  const theme = ref<AppTheme>(readStored(THEME_KEY, ALLOWED_THEMES, "system"));
  const defaultTab = ref(readStored(DEFAULT_TAB_KEY, ALLOWED_TABS as unknown as string[], "Workflows"));

  // apply theme immediately on store init so the DOM is styled before first render.
  applyTheme(theme.value);

  watch(theme, (next) => {
    applyTheme(next);
    writeStored(THEME_KEY, next);
  });

  watch(defaultTab, (next) => {
    writeStored(DEFAULT_TAB_KEY, next);
  });

  function setTheme(next: AppTheme) {
    theme.value = next;
  }

  function setDefaultTab(next: string) {
    defaultTab.value = next;
  }

  return { theme, defaultTab, setTheme, setDefaultTab };
});
