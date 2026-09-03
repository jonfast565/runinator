import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ExecutionProfile, ExecutionProfileInput } from "../../domain/models";
import type { AppService } from "../app";
import { createExecutionProfilesService } from "../execution-profiles";

vi.mock("../../api/commandCenterApi", () => ({
  fetchExecutionProfiles: vi.fn(),
  putExecutionProfile: vi.fn(),
  deleteExecutionProfile: vi.fn(),
  rotateExecutionProfile: vi.fn(),
  testExecutionProfile: vi.fn(),
}));

import {
  deleteExecutionProfile,
  fetchExecutionProfiles,
  putExecutionProfile,
} from "../../api/commandCenterApi";

const input: ExecutionProfileInput = {
  name: "github-default",
  description: "GitHub login",
  credential_scopes: ["github"],
  collection: {
    version: 1,
    sources: [{ type: "file", path: "~/.gitconfig", target: ".gitconfig" }],
  },
  exposure: { version: 1, home_overlay: true, environment: {} },
  enabled: true,
};

const profile: ExecutionProfile = {
  ...input,
  id: "profile-1",
  org_id: "org-1",
  config_version: 1,
  config_digest: "digest",
  current_revision: null,
  current_digest: null,
  current_publisher_id: null,
  published_at: null,
  expires_at: null,
  refresh_requested_at: null,
  health: "unpublished",
  last_error: null,
  created_at: "2026-09-03T00:00:00Z",
  updated_at: "2026-09-03T00:00:00Z",
};

const app = {
  runOperation: <T>(_label: string, run: () => Promise<T>) => run(),
} as AppService;

describe("execution-profile service", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    vi.mocked(fetchExecutionProfiles).mockResolvedValue([profile]);
  });

  it("refreshes and clears backend state", async () => {
    const service = createExecutionProfilesService(app);
    await service.refresh();
    expect(service.getState().profiles).toEqual([profile]);
    service.clear();
    expect(service.getState().profiles).toEqual([]);
  });

  it("routes mutations through the API and refreshes", async () => {
    const service = createExecutionProfilesService(app);
    await service.save(profile.id, input);
    expect(putExecutionProfile).toHaveBeenCalledWith(profile.id, input);
    await service.remove(profile.id);
    expect(deleteExecutionProfile).toHaveBeenCalledWith(profile.id);
    expect(fetchExecutionProfiles).toHaveBeenCalledTimes(2);
  });
});
