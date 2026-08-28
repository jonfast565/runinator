import { beforeEach, describe, expect, it, vi } from "vitest";
import { createOrgsService } from "../orgs";
import type { AppService } from "../app";
import type { AuthService } from "../auth";
import type { OrgMembershipView } from "../../api/commandCenterApi";

vi.mock("../../api/commandCenterApi", () => ({
  createOrg: vi.fn(),
  listMyOrgs: vi.fn(),
  switchOrg: vi.fn(),
}));

import { listMyOrgs, switchOrg } from "../../api/commandCenterApi";

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

const app = {
  runOperation: <T>(_label: string, run: () => Promise<T>) => run(),
  setError: vi.fn(),
  setStatus: vi.fn(),
} as unknown as AppService;

const applyAccessToken = vi.fn();
const reloadMe = vi.fn();
const auth = {
  applyAccessToken,
  reloadMe,
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
  });

  it("selects the first server-provided org when a frontend session begins", async () => {
    const service = createOrgsService(app, auth);
    service.setActiveLocal("acme");

    await service.refresh({ selectDefault: true });

    expect(switchOrg).toHaveBeenCalledWith("platform");
    expect(service.getState().activeOrgId).toBe("platform");
    expect(service.activeOrg()?.name).toBe("Platform");
  });
});
