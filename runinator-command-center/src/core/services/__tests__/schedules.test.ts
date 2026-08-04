import { beforeEach, describe, expect, it, vi } from "vitest";
import { createSchedulesService } from "../schedules";
import type { FreezeWindow, NewFreezeWindow } from "../../domain/models";
import type { AppService } from "../app";

vi.mock("../../api/commandCenterApi", () => ({
  fetchFreezeWindows: vi.fn(),
  createFreezeWindow: vi.fn(),
  updateFreezeWindow: vi.fn(),
  deleteFreezeWindow: vi.fn(),
}));

import {
  createFreezeWindow,
  deleteFreezeWindow,
  fetchFreezeWindows,
  updateFreezeWindow,
} from "../../api/commandCenterApi";

const hour = 60 * 60 * 1000;

function window(overrides: Partial<FreezeWindow> = {}): FreezeWindow {
  return {
    id: "window-1",
    org_id: null,
    workflow_id: null,
    name: "change freeze",
    reason: null,
    starts_at: new Date(Date.now() - hour).toISOString(),
    ends_at: new Date(Date.now() + hour).toISOString(),
    enabled: true,
    created_at: "2026-08-05T00:00:00Z",
    updated_at: "2026-08-05T00:00:00Z",
    ...overrides,
  };
}

const draft: NewFreezeWindow = {
  org_id: null,
  workflow_id: null,
  name: "change freeze",
  reason: null,
  starts_at: "2026-08-05T00:00:00Z",
  ends_at: "2026-08-06T00:00:00Z",
  enabled: true,
};

const errors: string[] = [];

// a minimal app stub: runOperation passes the call through so rejections reach the service.
const app = {
  runOperation: <T>(_label: string, run: () => Promise<T>) => run(),
  setError: (message: string) => {
    errors.push(message);
  },
} as unknown as AppService;

describe("schedules service freeze windows", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    errors.length = 0;
    vi.mocked(fetchFreezeWindows).mockResolvedValue([window()]);
  });

  it("loads freeze windows into state", async () => {
    const service = createSchedulesService(app);
    await service.refreshFreezeWindows();
    expect(service.getState().freezeWindows).toHaveLength(1);
  });

  it("counts only the windows in effect right now", async () => {
    const service = createSchedulesService(app);
    vi.mocked(fetchFreezeWindows).mockResolvedValue([
      window(),
      // an upcoming window is not suppressing anything yet.
      window({
        id: "window-2",
        starts_at: new Date(Date.now() + hour).toISOString(),
        ends_at: new Date(Date.now() + 2 * hour).toISOString(),
      }),
      // a disabled window covering now still freezes nothing.
      window({ id: "window-3", enabled: false }),
    ]);
    await service.refreshFreezeWindows();
    expect(service.activeCount()).toBe(1);
  });

  it("creates when no id is supplied and updates when one is", async () => {
    const service = createSchedulesService(app);
    vi.mocked(createFreezeWindow).mockResolvedValue(window());
    vi.mocked(updateFreezeWindow).mockResolvedValue(window());

    expect(await service.saveFreezeWindow(draft)).toBe(true);
    expect(createFreezeWindow).toHaveBeenCalledWith(draft);
    expect(updateFreezeWindow).not.toHaveBeenCalled();

    expect(await service.saveFreezeWindow(draft, "window-1")).toBe(true);
    expect(updateFreezeWindow).toHaveBeenCalledWith("window-1", draft);
  });

  it("surfaces a rejected range instead of reporting success", async () => {
    const service = createSchedulesService(app);
    vi.mocked(createFreezeWindow).mockRejectedValue(
      new Error("RUNI144 - ends_at must be after starts_at"),
    );

    // a window that silently froze nothing would only be discovered during the freeze.
    expect(await service.saveFreezeWindow(draft)).toBe(false);
    expect(errors.join()).toContain("ends_at must be after starts_at");
    expect(fetchFreezeWindows).not.toHaveBeenCalled();
  });

  it("reloads after a successful delete and reports a failed one", async () => {
    const service = createSchedulesService(app);
    vi.mocked(deleteFreezeWindow).mockResolvedValue({ success: true, message: "deleted" });

    expect(await service.removeFreezeWindow("window-1")).toBe(true);
    expect(fetchFreezeWindows).toHaveBeenCalledTimes(1);

    vi.mocked(deleteFreezeWindow).mockRejectedValue(new Error("boom"));
    expect(await service.removeFreezeWindow("window-1")).toBe(false);
    // a failed delete must not trigger a reload that would imply it worked.
    expect(fetchFreezeWindows).toHaveBeenCalledTimes(1);
  });
});
