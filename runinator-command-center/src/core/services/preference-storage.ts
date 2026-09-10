/** Non-secret, browser-local preferences. */
export interface PreferenceStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
  keys(): string[];
}

export const browserPreferences: PreferenceStorage = {
  getItem(key) {
    try {
      return localStorage.getItem(key);
    } catch {
      return null;
    }
  },
  setItem(key, value) {
    try {
      localStorage.setItem(key, value);
    } catch {
      /* memory-only. */
    }
  },
  removeItem(key) {
    try {
      localStorage.removeItem(key);
    } catch {
      /* memory-only. */
    }
  },
  keys() {
    try {
      return Array.from({ length: localStorage.length }, (_, i) => localStorage.key(i)).filter(
        (key): key is string => key !== null,
      );
    } catch {
      return [];
    }
  },
};
