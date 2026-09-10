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

  it("shows every timeline category by default and persists hidden categories", () => {
    const storage = storageMock();
    vi.stubGlobal("localStorage", storage);
    const preferences = createDisplayPreferencesService();

    expect(preferences.getState().hiddenTimelineEventCategories).toEqual([]);

    preferences.setTimelineEventCategoryVisible("system", false);

    expect(preferences.getState().hiddenTimelineEventCategories).toEqual(["system"]);
    expect(storage.setItem).toHaveBeenCalledWith(
      "command-center.timeline.hiddenCategories",
      '["system"]',
    );
  });

  it("restores generic hidden timeline categories", () => {
    vi.stubGlobal(
      "localStorage",
      storageMock({ "command-center.timeline.hiddenCategories": '["system","orchestration"]' }),
    );

    expect(createDisplayPreferencesService().getState().hiddenTimelineEventCategories).toEqual([
      "system",
      "orchestration",
    ]);
  });

  it("migrates the legacy system visibility preference", () => {
    vi.stubGlobal(
      "localStorage",
      storageMock({ "command-center.timeline.showSystemEvents": "false" }),
    );

    expect(createDisplayPreferencesService().getState().hiddenTimelineEventCategories).toEqual([
      "system",
    ]);
  });
});
