import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { usePermissionsStore } from "../permissions";

vi.mock("../../api/commandCenterApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../api/commandCenterApi")>()),
  addTeamMember: vi.fn(),
  createApiKey: vi.fn(),
  listApiKeys: vi.fn(),
  listTeams: vi.fn(),
  listTeamMembers: vi.fn(),
  listUserTeams: vi.fn(),
  listUsers: vi.fn(),
  revokeApiKey: vi.fn(),
  rotateApiKey: vi.fn(),
  updateApiKey: vi.fn(),
}));

import {
  addTeamMember,
  createApiKey,
  listApiKeys,
  listTeams,
  listTeamMembers,
  listUserTeams,
  listUsers,
  revokeApiKey,
  rotateApiKey,
  updateApiKey,
} from "../../api/commandCenterApi";

describe("permissions store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
    vi.stubGlobal("window", {
      clearTimeout: vi.fn(),
      setTimeout: vi.fn(),
    });
    vi.mocked(listApiKeys).mockResolvedValue([
      {
        id: "k-1",
        name: "ada key",
        user_id: "u-1",
        is_service: false,
        key_prefix: "ada",
        last_used_at: null,
        expires_at: null,
        disabled: false,
        created_at: "2026-01-01T00:00:00Z",
      },
      {
        id: "k-2",
        name: "service key",
        user_id: null,
        is_service: true,
        key_prefix: "svc",
        last_used_at: null,
        expires_at: null,
        disabled: false,
        created_at: "2026-01-01T00:00:00Z",
      },
    ]);
    vi.mocked(listUsers).mockResolvedValue([
      {
        id: "u-1",
        username: "ada",
        email: "ada@example.com",
        is_admin: true,
        disabled: false,
        created_at: "2026-01-01T00:00:00Z",
        updated_at: "2026-01-01T00:00:00Z",
      },
    ]);
    vi.mocked(listTeams).mockResolvedValue([
      {
        id: "t-1",
        name: "platform",
        created_at: "2026-01-01T00:00:00Z",
      },
    ]);
    vi.mocked(listUserTeams).mockResolvedValue([]);
    vi.mocked(listTeamMembers).mockResolvedValue([]);
    vi.mocked(addTeamMember).mockResolvedValue({ success: true, message: "Member added" });
    vi.mocked(createApiKey).mockResolvedValue({
      api_key: {
        id: "k-3",
        name: "new key",
        user_id: "u-1",
        is_service: false,
        key_prefix: "new",
        last_used_at: null,
        expires_at: null,
        disabled: false,
        created_at: "2026-01-02T00:00:00Z",
      },
      secret: "new.secret",
    });
    vi.mocked(updateApiKey).mockResolvedValue({
      id: "k-1",
      name: "renamed key",
      user_id: "u-1",
      is_service: false,
      key_prefix: "ada",
      last_used_at: null,
      expires_at: null,
      disabled: false,
      created_at: "2026-01-01T00:00:00Z",
    });
    vi.mocked(revokeApiKey).mockResolvedValue({ success: true, message: "API key revoked" });
    vi.mocked(rotateApiKey).mockResolvedValue({
      api_key: {
        id: "k-4",
        name: "ada key",
        user_id: "u-1",
        is_service: false,
        key_prefix: "rotated",
        last_used_at: null,
        expires_at: null,
        disabled: false,
        created_at: "2026-01-03T00:00:00Z",
      },
      secret: "rotated.secret",
    });
  });

  it("hydrates users and teams", async () => {
    const permissions = usePermissionsStore();

    await permissions.refreshAll();

    expect(permissions.users).toHaveLength(1);
    expect(permissions.teams).toHaveLength(1);
    expect(permissions.apiKeys).toHaveLength(2);
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

  it("filters selected user api keys with service keys", async () => {
    const permissions = usePermissionsStore();
    await permissions.refreshAll();
    permissions.selectUser(permissions.users[0]);

    expect(permissions.visibleApiKeys.map((key) => key.id)).toEqual(["k-1", "k-2"]);
  });

  it("creates an api key and preserves the one-time secret", async () => {
    const permissions = usePermissionsStore();
    await permissions.refreshAll();
    permissions.selectUser(permissions.users[0]);
    permissions.apiKeyDraft.name = "new key";

    await permissions.saveApiKeyDraft();

    expect(createApiKey).toHaveBeenCalledWith({
      name: "new key",
      is_service: false,
      user_id: "u-1",
      expires_at: null,
    });
    expect(permissions.revealedApiKey?.secret).toBe("new.secret");
  });

  it("updates, revokes, and rotates the selected api key", async () => {
    const permissions = usePermissionsStore();
    await permissions.refreshAll();
    permissions.selectApiKey(permissions.apiKeys[0]);
    permissions.apiKeyDraft.name = "renamed key";

    await permissions.saveApiKeyDraft();
    expect(updateApiKey).toHaveBeenCalledWith("k-1", {
      name: "renamed key",
      expires_at: null,
      disabled: false,
    });

    await permissions.revokeSelectedApiKey();
    expect(revokeApiKey).toHaveBeenCalledWith("k-1");

    permissions.selectApiKey(permissions.apiKeys[0]);
    await permissions.rotateSelectedApiKey();
    expect(rotateApiKey).toHaveBeenCalledWith("k-1");
    expect(permissions.revealedApiKey?.secret).toBe("rotated.secret");
  });
});
