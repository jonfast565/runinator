import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { usePermissionsStore } from "../permissions";

vi.mock("../../api/commandCenterApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../api/commandCenterApi")>()),
  addTeamMember: vi.fn(),
  listApiKeys: vi.fn(),
  listTeams: vi.fn(),
  listTeamMembers: vi.fn(),
  listUserTeams: vi.fn(),
  listUsers: vi.fn()
}));

import {
  addTeamMember,
  listApiKeys,
  listTeams,
  listTeamMembers,
  listUserTeams,
  listUsers
} from "../../api/commandCenterApi";

describe("permissions store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
    vi.stubGlobal("window", {
      clearTimeout: vi.fn(),
      setTimeout: vi.fn()
    });
    vi.mocked(listApiKeys).mockResolvedValue([]);
    vi.mocked(listUsers).mockResolvedValue([
      {
        id: "u-1",
        username: "ada",
        email: "ada@example.com",
        is_admin: true,
        disabled: false,
        created_at: "2026-01-01T00:00:00Z",
        updated_at: "2026-01-01T00:00:00Z"
      }
    ]);
    vi.mocked(listTeams).mockResolvedValue([
      {
        id: "t-1",
        name: "platform",
        created_at: "2026-01-01T00:00:00Z"
      }
    ]);
    vi.mocked(listUserTeams).mockResolvedValue([]);
    vi.mocked(listTeamMembers).mockResolvedValue([]);
    vi.mocked(addTeamMember).mockResolvedValue({ success: true, message: "Member added" });
  });

  it("hydrates users and teams", async () => {
    const permissions = usePermissionsStore();

    await permissions.refreshAll();

    expect(permissions.users).toHaveLength(1);
    expect(permissions.teams).toHaveLength(1);
    expect(permissions.enabledAdminCount).toBe(1);
  });

  it("assigns the selected user to a team", async () => {
    const permissions = usePermissionsStore();
    await permissions.refreshAll();
    permissions.selectUser(permissions.users[0]);

    await permissions.assignSelectedUserToTeam("t-1");

    expect(addTeamMember).toHaveBeenCalledWith("t-1", "u-1");
    expect(listUserTeams).toHaveBeenCalledWith("u-1");
  });
});
