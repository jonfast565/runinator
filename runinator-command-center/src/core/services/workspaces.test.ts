import { describe, expect, it, vi } from "vitest";
import type { DurableWorkspace, WorkspaceSnapshot } from "../domain/models/workspaces";
vi.mock("../api/commandCenterApi", () => ({
  fetchDurableWorkspaces: vi.fn(),
  fetchWorkspaceVersions: vi.fn(),
  fetchWorkspaceSnapshot: vi.fn(),
  fetchWorkspaceDirectory: vi.fn(),
  fetchWorkspaceDiff: vi.fn(),
  importWorkspaceArchive: vi.fn(),
  createWorkspaceTransfer: vi.fn(),
  fetchWorkspaceTransfer: vi.fn(),
  cancelWorkspaceTransfer: vi.fn(),
  previewWorkspaceFile: vi.fn(),
  deleteDurableWorkspace: vi.fn(),
  downloadWorkspaceVersion: vi.fn(),
}));
import {
  fetchDurableWorkspaces,
  fetchWorkspaceVersions,
  fetchWorkspaceSnapshot,
  fetchWorkspaceDirectory,
  previewWorkspaceFile,
} from "../api/commandCenterApi";
import { createWorkspacesService } from "./workspaces";

function pending<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

describe("workspace navigation", () => {
  it("keeps a pinned version outside the loaded history page", async () => {
    vi.mocked(fetchWorkspaceVersions).mockResolvedValueOnce([
      { version: 100 } as WorkspaceSnapshot,
    ]);
    vi.mocked(fetchWorkspaceSnapshot).mockResolvedValueOnce({ version: 2 } as WorkspaceSnapshot);
    const service = createWorkspacesService();
    await service.select({ id: "selected" } as DurableWorkspace, 0, 2);
    expect(service.getState().pinnedSnapshot?.version).toBe(2);
  });

  it("allows list refresh and version loading to finish independently", async () => {
    const versions = pending<WorkspaceSnapshot[]>();
    vi.mocked(fetchWorkspaceVersions).mockReturnValueOnce(versions.promise);
    vi.mocked(fetchDurableWorkspaces).mockResolvedValueOnce([]);
    const service = createWorkspacesService();
    const selection = service.select({ id: "selected" } as DurableWorkspace);
    await service.refresh();
    versions.resolve([{ version: 3 } as WorkspaceSnapshot]);
    await selection;
    expect(service.getState().versions[0]?.version).toBe(3);
  });
  it("invalidates previews when the visible revision changes", async () => {
    const preview = pending<string>();
    vi.mocked(previewWorkspaceFile).mockReturnValueOnce(preview.promise);
    vi.mocked(fetchWorkspaceDirectory).mockResolvedValueOnce({
      revision_id: "new",
      path: "",
      entries: [],
      next_cursor: null,
    });
    const service = createWorkspacesService();
    const loading = service.previewFile("workspace", 1, "file");
    await service.browse("workspace", 2);
    preview.resolve("old data");
    await loading;
    expect(service.getState().preview).toBe("");
    expect(service.getState().directory?.revision_id).toBe("new");
  });
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
