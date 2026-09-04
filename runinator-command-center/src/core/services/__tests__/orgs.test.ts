import { beforeEach, describe, expect, it, vi } from "vitest";
import { createOrgsService } from "../orgs";
import type { AppService } from "../app";
import type { AuthService } from "../auth";
import type { OrgMembershipView } from "../../api/commandCenterApi";

vi.mock("../../api/commandCenterApi", () => ({
  createOrg: vi.fn(),
  listMyOrgs: vi.fn(),
  switchOrg: vi.fn(),
  switchPlatform: vi.fn(),
  updateOrg: vi.fn(),
}));

import { listMyOrgs, switchOrg, switchPlatform, updateOrg } from "../../api/commandCenterApi";

function membership(id: string, name: string): OrgMembershipView {
  return {
    org: {
      id,
      name,
      slug: name.toLowerCase(),
      disabled: false,
      created_at: "2026-08-28T00:00:00Z",
      updated_at: "2026-08-28T00:00:00Z",
    },
    role: "owner",
  };
}

const setError = vi.fn();
const setStatus = vi.fn();
const app = {
  runOperation: <T>(_label: string, run: () => Promise<T>) => run(),
  setError,
  setStatus,
} as unknown as AppService;

const applyAccessToken = vi.fn();
const reloadMe = vi.fn();
const getState = vi.fn();
const registerScopeRestorer = vi.fn();
const auth = {
  applyAccessToken,
  reloadMe,
  getState,
  registerScopeRestorer,
} as unknown as AuthService;

describe("organizations service", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    vi.mocked(listMyOrgs).mockResolvedValue([
      membership("platform", "Platform"),
      membership("acme", "Acme"),
    ]);
    vi.mocked(switchOrg).mockResolvedValue({
      access_token: "scoped-token",
      expires_in: 3600,
      org: membership("platform", "Platform").org,
      role: "owner",
    });
    applyAccessToken.mockResolvedValue(undefined);
    reloadMe.mockResolvedValue(undefined);
    getState.mockReturnValue({ required: true });
  });

  it("selects the first server-provided org when a frontend session begins", async () => {
    const service = createOrgsService(app, auth);
    service.setActiveLocal("acme");

    await service.refresh({ selectDefault: true });

    expect(switchOrg).toHaveBeenCalledWith("platform");
    expect(service.getState().activeOrgId).toBe("platform");
    expect(service.activeOrg()?.name).toBe("Platform");
  });

  it.each([{ memberships: [] }, { memberships: [membership("acme", "Acme")] }])(
    "keeps platform admins in the implicit scope with memberships %j",
    async ({ memberships }) => {
      getState.mockReturnValue({ required: true, user: { platform_role: "admin" } });
      vi.mocked(listMyOrgs).mockResolvedValue(memberships);
      const service = createOrgsService(app, auth);
      await service.refresh({ selectDefault: true });
      expect(switchOrg).not.toHaveBeenCalled();
      expect(service.getState().activeOrgId).toBeNull();
    },
  );

  it("returns a platform-capable user to the org-less platform scope", async () => {
    const service = createOrgsService(app, auth);
    service.setActiveLocal("acme");

    vi.mocked(switchPlatform).mockResolvedValue({
      access_token: "platform-token",
      expires_in: 3600,
    });

    await service.setActivePlatform();

    expect(switchPlatform).toHaveBeenCalledOnce();
    expect(applyAccessToken).toHaveBeenCalledWith("platform-token");
    expect(service.getState().activeOrgId).toBeNull();
    expect(setStatus).toHaveBeenCalledWith("Active scope: Platform");
  });

  it("renames the active organization in local memberships", async () => {
    const service = createOrgsService(app, auth);
    await service.refresh({ selectDefault: true });
    vi.mocked(updateOrg).mockResolvedValue({
      ...membership("platform", "Runinator").org,
      slug: "platform",
    });

    await expect(service.rename("Runinator")).resolves.toBe(true);

    expect(updateOrg).toHaveBeenCalledWith("platform", "Runinator");
    expect(service.activeOrg()?.name).toBe("Runinator");
    expect(service.activeOrg()?.slug).toBe("platform");
  });
});
