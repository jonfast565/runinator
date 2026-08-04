import { beforeEach, describe, expect, it, vi } from "vitest";
import { createNotificationsService } from "../notifications";
import type { NewNotificationPolicy, NotificationPolicy } from "../../domain/models";
import type { AppService } from "../app";

vi.mock("../../api/commandCenterApi", () => ({
  fetchNotifications: vi.fn(),
  markNotificationRead: vi.fn(),
  markAllNotificationsRead: vi.fn(),
  deleteNotification: vi.fn(),
  fetchNotificationPolicies: vi.fn(),
  createNotificationPolicy: vi.fn(),
  updateNotificationPolicy: vi.fn(),
  deleteNotificationPolicy: vi.fn(),
}));

import {
  createNotificationPolicy,
  deleteNotificationPolicy,
  fetchNotificationPolicies,
  updateNotificationPolicy,
} from "../../api/commandCenterApi";

const policy: NotificationPolicy = {
  id: "policy-1",
  workflow_id: null,
  name: "oncall",
  event: "run_failed",
  severity: "critical",
  channel: "slack",
  target: "#oncall",
  threshold_seconds: null,
  enabled: true,
  managed_by: null,
  configuration: null,
  created_at: "2026-08-04T00:00:00Z",
  updated_at: "2026-08-04T00:00:00Z",
};

const draft: NewNotificationPolicy = {
  workflow_id: null,
  name: "oncall",
  event: "run_failed",
  severity: "critical",
  channel: "slack",
  target: "#oncall",
  threshold_seconds: null,
  enabled: true,
  managed_by: null,
  configuration: null,
};

const errors: string[] = [];

// a minimal app stub: runOperation passes the call through so rejections reach the service.
const app = {
  runOperation: <T>(_label: string, run: () => Promise<T>) => run(),
  setError: (message: string) => {
    errors.push(message);
  },
} as unknown as AppService;

describe("notifications service policies", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    errors.length = 0;
    vi.mocked(fetchNotificationPolicies).mockResolvedValue([policy]);
  });

  it("loads policies into state", async () => {
    const service = createNotificationsService(app);
    await service.refreshPolicies();
    expect(service.getState().policies).toEqual([policy]);
  });

  it("creates when no id is supplied and updates when one is", async () => {
    const service = createNotificationsService(app);
    vi.mocked(createNotificationPolicy).mockResolvedValue(policy);
    vi.mocked(updateNotificationPolicy).mockResolvedValue(policy);

    expect(await service.savePolicy(draft)).toBe(true);
    expect(createNotificationPolicy).toHaveBeenCalledWith(draft);
    expect(updateNotificationPolicy).not.toHaveBeenCalled();

    expect(await service.savePolicy(draft, "policy-1")).toBe(true);
    expect(updateNotificationPolicy).toHaveBeenCalledWith("policy-1", draft);
  });

  it("surfaces a backend validation rejection instead of reporting success", async () => {
    const service = createNotificationsService(app);
    vi.mocked(createNotificationPolicy).mockRejectedValue(
      new Error("RUNI143 - channel 'slack' requires a target"),
    );

    // the operator must see why the policy was refused; silently succeeding would leave them
    // believing an alert is configured when none is.
    expect(await service.savePolicy(draft)).toBe(false);
    expect(errors.join()).toContain("requires a target");
    expect(fetchNotificationPolicies).not.toHaveBeenCalled();
  });

  it("reloads after a successful delete and reports a failed one", async () => {
    const service = createNotificationsService(app);
    vi.mocked(deleteNotificationPolicy).mockResolvedValue({
      success: true,
      message: "deleted",
    });

    expect(await service.removePolicy("policy-1")).toBe(true);
    expect(fetchNotificationPolicies).toHaveBeenCalledTimes(1);

    vi.mocked(deleteNotificationPolicy).mockRejectedValue(new Error("boom"));
    expect(await service.removePolicy("policy-1")).toBe(false);
    // a failed delete must not trigger a reload that would imply it worked.
    expect(fetchNotificationPolicies).toHaveBeenCalledTimes(1);
  });
});
