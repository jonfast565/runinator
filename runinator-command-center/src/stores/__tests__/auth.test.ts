import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { authService } from "../../core/services";
import { useAuthStore } from "../auth";

vi.mock("../../core/api/commandCenterApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../core/api/commandCenterApi")>()),
  fetchAuthConfig: vi.fn(),
  fetchAuthMe: vi.fn(),
  login: vi.fn(),
  logout: vi.fn(),
  refreshSession: vi.fn(),
  setAccessToken: vi.fn(),
}));

import {
  fetchAuthConfig,
  fetchAuthMe,
  refreshSession,
  setAccessToken,
} from "../../core/api/commandCenterApi";

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

describe("auth store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    authService.resetForTests();
    vi.clearAllMocks();
  });

  it("treats auth-disabled stacks as immediately authenticated", async () => {
    vi.mocked(fetchAuthConfig).mockResolvedValue({ enabled: false });
    vi.stubGlobal("localStorage", storageMock());

    const auth = useAuthStore();
    await auth.init();

    expect(auth.required).toBe(false);
    expect(auth.authenticated).toBe(true);
    expect(auth.ready).toBe(true);
    expect(fetchAuthMe).not.toHaveBeenCalled();
    expect(setAccessToken).not.toHaveBeenCalled();
  });

  it("refreshes the session when a persisted access token is stale", async () => {
    vi.mocked(fetchAuthConfig).mockResolvedValue({ enabled: true });
    vi.mocked(fetchAuthMe).mockRejectedValue(new Error("stale access"));
    vi.mocked(refreshSession).mockResolvedValue({
      access_token: "fresh-access",
      refresh_token: "fresh-refresh",
      expires_in: 3600,
      user: { id: "u-1", username: "admin" },
    });
    const { getPlatformAdapter } = await import("../../core/platform");
    getPlatformAdapter().authStorage.set("runinator.auth.access", "stale-access");
    getPlatformAdapter().authStorage.set("runinator.auth.refresh", "refresh-1");

    const auth = useAuthStore();
    await auth.init();

    expect(fetchAuthMe).toHaveBeenCalledTimes(1);
    expect(refreshSession).toHaveBeenCalledWith("refresh-1");
    expect(setAccessToken).toHaveBeenNthCalledWith(1, "stale-access");
    expect(setAccessToken).toHaveBeenNthCalledWith(2, "fresh-access");
    expect(auth.required).toBe(true);
    expect(auth.authenticated).toBe(true);
    expect(auth.user).toEqual({ id: "u-1", username: "admin" });
    expect(auth.ready).toBe(true);
  });
});
