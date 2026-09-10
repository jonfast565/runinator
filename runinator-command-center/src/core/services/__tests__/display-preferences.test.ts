import { beforeEach, describe, expect, it, vi } from "vitest";

import { createDisplayPreferencesService } from "../display-preferences";

function storageMock(seed: Record<string, string> = {}) {
  const data = new Map(Object.entries(seed));
  return {
    getItem: vi.fn((key: string) => data.get(key) ?? null),
    setItem: vi.fn((key: string, value: string) => {
      data.set(key, value);
    }),
    removeItem: vi.fn((key: string) => {
      data.delete(key);
    }),
  };
}

describe("display preferences", () => {
  beforeEach(() => {
    vi.unstubAllGlobals();
  });

  it("shows system timeline events by default and persists changes", () => {
    const storage = storageMock();
    vi.stubGlobal("localStorage", storage);
    const preferences = createDisplayPreferencesService();

    expect(preferences.getState().showSystemTimelineEvents).toBe(true);

    preferences.setShowSystemTimelineEvents(false);

    expect(preferences.getState().showSystemTimelineEvents).toBe(false);
    expect(storage.setItem).toHaveBeenCalledWith(
      "command-center.timeline.showSystemEvents",
      "false",
    );
  });

  it("restores the system timeline preference", () => {
    vi.stubGlobal(
      "localStorage",
      storageMock({ "command-center.timeline.showSystemEvents": "false" }),
    );

    expect(createDisplayPreferencesService().getState().showSystemTimelineEvents).toBe(false);
  });
});
