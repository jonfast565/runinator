import { describe, expect, it, vi } from "vitest";
import type { DurableWorkspace, WorkspaceSnapshot } from "../domain/models/workspaces";
vi.mock("../api/commandCenterApi", () => ({
  fetchDurableWorkspaces: vi.fn(),
  fetchWorkspaceVersions: vi.fn(),
  deleteDurableWorkspace: vi.fn(),
  downloadWorkspaceVersion: vi.fn(),
}));
import { fetchDurableWorkspaces, fetchWorkspaceVersions } from "../api/commandCenterApi";
import { createWorkspacesService } from "./workspaces";

function pending<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

describe("workspace navigation", () => {
  it("does not leak a previous organization's late response after logout", async () => {
    const response = pending<DurableWorkspace[]>();
    vi.mocked(fetchDurableWorkspaces).mockReturnValueOnce(response.promise);
    const service = createWorkspacesService();
    const loading = service.refresh();
    service.clear();
    response.resolve([{ id: "old" } as DurableWorkspace]);
    await loading;
    expect(service.getState().items).toEqual([]);
  });
  it("keeps the latest selection when version requests finish out of order", async () => {
    const first = pending<WorkspaceSnapshot[]>();
    vi.mocked(fetchWorkspaceVersions).mockReturnValueOnce(first.promise).mockResolvedValueOnce([]);
    const service = createWorkspacesService();
    const loading = service.select({ id: "first" } as DurableWorkspace);
    await service.select({ id: "second" } as DurableWorkspace);
    first.resolve([{ version: 99 } as WorkspaceSnapshot]);
    await loading;
    expect(service.getState().selected?.id).toBe("second");
    expect(service.getState().versions).toEqual([]);
  });
});
