import { createAppService } from "../app";
import { describe, it, expect } from "vitest";
import { createDisplayPreferencesService } from "../display-preferences";
import { WatchExpressionStorage } from "../workflows/run-watches";
import type { PreferenceStorage } from "../preference-storage";

function storage(): PreferenceStorage {
  const values = new Map<string, string>();
  return {
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => {
      values.set(key, value);
    },
    removeItem: (key) => {
      values.delete(key);
    },
    keys: () => [...values.keys()],
  };
}

describe("injected preferences", () => {
  it("isolates clients and reloads preferences from the selected store", () => {
    const first = storage(),
      second = storage();
    createDisplayPreferencesService(first).setTheme("dark");
    expect(createDisplayPreferencesService(first).getState().theme).toBe("dark");
    expect(createDisplayPreferencesService(second).getState().theme).toBe("system");
  });
  it("shares default-tab preferences with the app and isolates sidebar state", () => {
    const local = storage();
    createDisplayPreferencesService(local).setDefaultTab("Runs");
    const app = createAppService(undefined, local);
    expect(app.getState().activeTab).toBe("Runs");
    app.toggleSidebar();
    expect(createAppService(undefined, local).getState().sidebarCollapsed).toBe(true);
    expect(createAppService(undefined, storage()).getState().sidebarCollapsed).toBe(false);
  });
  it("loads only valid watch entries and preserves workflow isolation", () => {
    const local = storage();
    local.setItem("other", "[]");
    local.setItem("runinator.watch.bad", "invalid");
    local.setItem("runinator.watch.mixed", '["x",7]');
    const watches = new WatchExpressionStorage(local);
    watches.save("one", ["a"]);
    watches.save("two", ["b"]);
    expect(watches.loadAll()).toEqual({ mixed: ["x"], one: ["a"], two: ["b"] });
    expect(new WatchExpressionStorage(storage()).loadAll()).toEqual({});
  });
});
