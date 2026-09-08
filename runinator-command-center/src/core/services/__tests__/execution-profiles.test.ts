import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  ExecutionProfile,
  ExecutionProfileCollectionStatus,
  ExecutionProfileInput,
} from "../../domain/models";
import type { AppService } from "../app";
import { createEventStreamRouter, type EventStreamRouterDeps } from "../../realtime/event-router";
import { createExecutionProfilesService } from "../execution-profiles";

vi.mock("../../api/commandCenterApi", () => ({
  fetchExecutionProfiles: vi.fn(),
  fetchExecutionProfileCollectionStatuses: vi.fn(),
  putExecutionProfile: vi.fn(),
  deleteExecutionProfile: vi.fn(),
  rotateExecutionProfile: vi.fn(),
  testExecutionProfile: vi.fn(),
}));

import {
  deleteExecutionProfile,
  fetchExecutionProfiles,
  fetchExecutionProfileCollectionStatuses,
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

const collectionStatus: ExecutionProfileCollectionStatus = {
  profile_id: profile.id,
  config_digest: profile.config_digest,
  publication_health: profile.health,
  current_revision: null,
  published_at: null,
  expires_at: null,
  latest_operation: null,
  agents: [],
};

describe("execution-profile service", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    vi.mocked(fetchExecutionProfiles).mockResolvedValue([profile]);
    vi.mocked(fetchExecutionProfileCollectionStatuses).mockResolvedValue([collectionStatus]);
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("refreshes and clears backend state", async () => {
    const service = createExecutionProfilesService(app);
    await service.refresh();
    expect(service.getState().profiles).toEqual([profile]);
    expect(service.getState().collectionStatuses).toEqual({ [profile.id]: collectionStatus });
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

describe("execution-profile live updates", () => {
  it("routes websocket events and coalesces a burst into a refresh of errors and publication", async () => {
    vi.useFakeTimers();
    vi.mocked(fetchExecutionProfiles).mockClear();
    vi.mocked(fetchExecutionProfileCollectionStatuses).mockClear();
    const failedProfile = { ...profile, health: "error" as const, last_error: "collector failed" };
    vi.mocked(fetchExecutionProfiles).mockResolvedValue([failedProfile]);
    vi.mocked(fetchExecutionProfileCollectionStatuses).mockResolvedValue([collectionStatus]);
    const service = createExecutionProfilesService(app);
    const router = createEventStreamRouter(
      () =>
        ({
          refreshExecutionProfilesIfActive: () => {
            service.scheduleCollectionStatusRefresh();
          },
        }) as EventStreamRouterDeps,
    );
    router.route({ type: "execution_profiles_changed" });
    router.route({ type: "execution_profiles_changed" });
    await vi.advanceTimersByTimeAsync(100);
    expect(fetchExecutionProfiles).toHaveBeenCalledTimes(1);
    expect(fetchExecutionProfileCollectionStatuses).toHaveBeenCalledTimes(1);
    expect(service.getState().profiles[0]?.last_error).toBe("collector failed");
    expect(service.getState().collectionStatuses[profile.id]).toEqual(collectionStatus);
  });

  it("discards in-flight responses after the authenticated scope is cleared", async () => {
    let resolveProfiles!: (profiles: ExecutionProfile[]) => void;
    vi.mocked(fetchExecutionProfiles).mockReturnValue(
      new Promise((resolve) => {
        resolveProfiles = resolve;
      }),
    );
    vi.mocked(fetchExecutionProfileCollectionStatuses).mockResolvedValue([collectionStatus]);
    const service = createExecutionProfilesService(app);
    const pending = service.refreshCollectionStatus();
    service.clear();
    resolveProfiles([profile]);
    await pending;
    expect(service.getState().profiles).toEqual([]);
    expect(service.getState().collectionStatuses).toEqual({});
  });
});

it("refetches after a websocket event arrives during an in-flight status request", async () => {
  let resolveProfiles!: (profiles: ExecutionProfile[]) => void;
  vi.mocked(fetchExecutionProfiles).mockReset();
  vi.mocked(fetchExecutionProfiles)
    .mockReturnValueOnce(
      new Promise((resolve) => {
        resolveProfiles = resolve;
      }),
    )
    .mockResolvedValue([{ ...profile, last_error: "new failure" }]);
  vi.mocked(fetchExecutionProfileCollectionStatuses).mockResolvedValue([collectionStatus]);
  const service = createExecutionProfilesService(app);
  const first = service.refreshCollectionStatus();
  const next = service.refreshCollectionStatus();
  resolveProfiles([profile]);
  await Promise.all([first, next]);
  expect(fetchExecutionProfiles).toHaveBeenCalledTimes(2);
  expect(service.getState().profiles[0]?.last_error).toBe("new failure");
});
