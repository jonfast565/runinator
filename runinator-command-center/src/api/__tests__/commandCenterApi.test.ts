import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  addTeamMember,
  createApiKey,
  createUser,
  fetchWorkflowNodeRunArtifacts,
  fetchWorkflowNodeRunChunks,
  listTeamMembers,
  rotateApiKey,
  updateApiKey,
  updateTeam,
} from "../commandCenterApi";
import { invoke } from "@tauri-apps/api/core";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

describe("command center workflow node run API", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockResolvedValue([]);
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
  });

  it("requests workflow node run chunks by node run id", async () => {
    await fetchWorkflowNodeRunChunks("00000000-0000-0000-0000-000000000042");

    expect(invoke).toHaveBeenCalledWith("fetch_workflow_node_run_chunks", {
      nodeRunId: "00000000-0000-0000-0000-000000000042",
    });
  });

  it("requests workflow node run artifacts by node run id", async () => {
    await fetchWorkflowNodeRunArtifacts("00000000-0000-0000-0000-000000000042");

    expect(invoke).toHaveBeenCalledWith("fetch_workflow_node_run_artifacts", {
      nodeRunId: "00000000-0000-0000-0000-000000000042",
    });
  });
});

describe("command center permissions API in web mode", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.stubGlobal("window", {});
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: true,
        status: 200,
        json: vi.fn().mockResolvedValue({}),
      }),
    );
  });

  it("maps user creation to the users endpoint", async () => {
    await createUser({
      username: "ada",
      password: "secret",
      email: "ada@example.com",
      is_admin: true,
    });

    expect(fetch).toHaveBeenCalledWith(
      "/api/users",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({
          username: "ada",
          password: "secret",
          email: "ada@example.com",
          is_admin: true,
        }),
      }),
    );
  });

  it("maps team rename and membership endpoints", async () => {
    await updateTeam("00000000-0000-0000-0000-000000000001", "platform");
    await addTeamMember(
      "00000000-0000-0000-0000-000000000001",
      "00000000-0000-0000-0000-000000000002",
    );
    await listTeamMembers("00000000-0000-0000-0000-000000000001");

    expect(fetch).toHaveBeenNthCalledWith(
      1,
      "/api/teams/00000000-0000-0000-0000-000000000001",
      expect.objectContaining({
        method: "PATCH",
        body: JSON.stringify({ name: "platform" }),
      }),
    );
    expect(fetch).toHaveBeenNthCalledWith(
      2,
      "/api/teams/00000000-0000-0000-0000-000000000001/members",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({ user_id: "00000000-0000-0000-0000-000000000002" }),
      }),
    );
    expect(fetch).toHaveBeenNthCalledWith(
      3,
      "/api/teams/00000000-0000-0000-0000-000000000001/members",
      expect.objectContaining({ method: "GET" }),
    );
  });

  it("maps api key lifecycle endpoints", async () => {
    await createApiKey({
      name: "deploy",
      user_id: "00000000-0000-0000-0000-000000000002",
      is_service: false,
      expires_at: null,
    });
    await updateApiKey("00000000-0000-0000-0000-000000000003", {
      name: "deploy renamed",
      expires_at: null,
      disabled: false,
    });
    await rotateApiKey("00000000-0000-0000-0000-000000000003");

    expect(fetch).toHaveBeenNthCalledWith(
      1,
      "/api/api_keys",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({
          name: "deploy",
          user_id: "00000000-0000-0000-0000-000000000002",
          is_service: false,
          expires_at: null,
        }),
      }),
    );
    expect(fetch).toHaveBeenNthCalledWith(
      2,
      "/api/api_keys/00000000-0000-0000-0000-000000000003",
      expect.objectContaining({
        method: "PATCH",
        body: JSON.stringify({
          name: "deploy renamed",
          expires_at: null,
          disabled: false,
        }),
      }),
    );
    expect(fetch).toHaveBeenNthCalledWith(
      3,
      "/api/api_keys/00000000-0000-0000-0000-000000000003/rotate",
      expect.objectContaining({ method: "POST" }),
    );
  });
});
