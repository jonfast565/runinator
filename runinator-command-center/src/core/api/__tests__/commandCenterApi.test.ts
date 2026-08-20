import { beforeEach, describe, expect, it, vi } from "vitest";
import { setCommandRuntime } from "../runtime";
import { apiBaseUrl, invokeViaHttp, wsBaseUrl } from "../httpRuntime";
import {
  addTeamMember,
  createApiKey,
  createUser,
  deliverSignal,
  fetchEnumCatalogs,
  fetchNodeKinds,
  fetchTriggerKinds,
  listTeamMembers,
  requestRunInterrupt,
  rotateApiKey,
  updateApiKey,
  updateTeam,
} from "../commandCenterApi";
import { invoke } from "@tauri-apps/api/core";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

describe("command center catalog metadata API", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockResolvedValue([]);
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
    setCommandRuntime({
      isTauri: () => true,
      invoke: (name, args) => invoke(name, args),
      wsBaseUrl: () => "http://127.0.0.1:8080",
      apiBaseUrl: () => "/api",
    });
  });

  it("requests node kinds", async () => {
    await fetchNodeKinds();
    expect(invoke).toHaveBeenCalledWith("fetch_node_kinds", undefined);
  });

  it("requests trigger kinds", async () => {
    await fetchTriggerKinds();
    expect(invoke).toHaveBeenCalledWith("fetch_trigger_kinds", undefined);
  });

  it("requests enum catalogs", async () => {
    await fetchEnumCatalogs();
    expect(invoke).toHaveBeenCalledWith("fetch_enum_catalogs", undefined);
  });
});

describe("command center permissions API in web mode", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.stubGlobal("window", {});
    setCommandRuntime({
      isTauri: () => false,
      invoke: invokeViaHttp,
      wsBaseUrl,
      apiBaseUrl,
    });
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
      platform_role: "admin",
    });

    expect(fetch).toHaveBeenCalledWith(
      "/api/users",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({
          username: "ada",
          password: "secret",
          email: "ada@example.com",
          platform_role: "admin",
        }),
      }),
    );
  });

  it("maps team rename and membership endpoints", async () => {
    await updateTeam("00000000-0000-0000-0000-000000000001", "platform");
    await addTeamMember(
      "00000000-0000-0000-0000-000000000001",
      "00000000-0000-0000-0000-000000000002",
      "member",
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
        body: JSON.stringify({ user_id: "00000000-0000-0000-0000-000000000002", role: "member" }),
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
      principal_kind: "user",
      principal_id: "00000000-0000-0000-0000-000000000002",
      action_ceiling: [],
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
          principal_kind: "user",
          principal_id: "00000000-0000-0000-0000-000000000002",
          action_ceiling: [],
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

describe("run side-channel endpoints in web mode", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.stubGlobal("window", {});
    setCommandRuntime({
      isTauri: () => false,
      invoke: invokeViaHttp,
      wsBaseUrl,
      apiBaseUrl,
    });
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: true,
        status: 200,
        json: vi.fn().mockResolvedValue({}),
      }),
    );
  });

  it("posts an interrupt request with its optional fields", async () => {
    await requestRunInterrupt(
      "00000000-0000-0000-0000-000000000080",
      "external",
      { why: "manual" },
      "00000000-0000-0000-0000-000000000081",
    );

    expect(fetch).toHaveBeenCalledWith(
      "/api/workflow_runs/00000000-0000-0000-0000-000000000080/interrupts",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({
          source: "external",
          payload: { why: "manual" },
          cursor_id: "00000000-0000-0000-0000-000000000081",
        }),
      }),
    );
  });

  /** signals had a tauri command but no http descriptor, so the web build threw on delivery. */
  it("posts a signal delivery", async () => {
    await deliverSignal("00000000-0000-0000-0000-000000000080", "approved", { by: "ada" });

    expect(fetch).toHaveBeenCalledWith(
      "/api/workflow_runs/00000000-0000-0000-0000-000000000080/signals",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({ name: "approved", payload: { by: "ada" } }),
      }),
    );
  });
});
