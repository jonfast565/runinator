import { describe, expect, it, vi } from "vitest";
import { createDirectoryLoader, type DirectoryState } from "./workspace-directories";
import type { WorkspacesApi } from "../api/ports/workspaces";
import type { WorkspaceDirectory, WorkspaceEntry } from "../domain/models/workspaces";

function entry(index: number): WorkspaceEntry {
  return {
    name: `file-${String(index).padStart(5, "0")}`,
    kind: "file",
    inode_number: index,
    content_id: String(index),
    size_bytes: index,
    executable: false,
    link_target: null,
  };
}

function directory(name: string): WorkspaceEntry {
  return { ...entry(0), name, kind: "directory" };
}

function setup(fetch: WorkspacesApi["fetchWorkspaceDirectory"]) {
  let state: Partial<Record<string, DirectoryState>> = {};
  const loader = createDirectoryLoader(
    { fetchWorkspaceDirectory: fetch } as WorkspacesApi,
    (next) => {
      state = next;
    },
  );
  return { loader, state: () => state };
}

describe("directory loading", () => {
  it.each([0, 1, 200, 201, 10000])(
    "loads all %i entries and reuses completed directories",
    async (count) => {
      const fetch = vi.fn(
        async (
          _w: string,
          _v: number,
          path = "",
          cursor: string | null = null,
        ): Promise<WorkspaceDirectory> => {
          const offset = Number(cursor ?? 0),
            end = Math.min(offset + 200, count);
          return {
            revision_id: "revision",
            path,
            entries: Array.from({ length: end - offset }, (_, i) => entry(offset + i)),
            next_cursor: end < count ? String(end) : null,
          };
        },
      );
      const { loader, state } = setup(fetch);
      await loader.load("workspace", 1, "");
      expect(state()[""]?.entries).toHaveLength(count);
      expect(state()[""]?.complete).toBe(true);
      const requests = fetch.mock.calls.length;
      await loader.load("workspace", 1, "");
      expect(fetch).toHaveBeenCalledTimes(requests);
      expect(state()[""]?.entries.at(-1)?.name).toBe(count ? entry(count - 1).name : undefined);
    },
  );
  it("retains a partial listing and retries the failed cursor without duplicates", async () => {
    const fetch = vi
      .fn()
      .mockResolvedValueOnce({
        revision_id: "r",
        path: "",
        entries: [entry(0)],
        next_cursor: "next",
      })
      .mockRejectedValueOnce(new Error("offline"))
      .mockResolvedValueOnce({
        revision_id: "r",
        path: "",
        entries: [entry(0), entry(1)],
        next_cursor: null,
      });
    const { loader, state } = setup(fetch);
    await loader.load("w", 1, "");
    expect(state()[""]?.entries).toHaveLength(1);
    expect(state()[""]?.error).toBe("offline");
    await loader.load("w", 1, "");
    expect(fetch.mock.calls[2]?.[3]).toBe("next");
    expect(state()[""]?.entries).toHaveLength(2);
  });
  it("deduplicates tree and selected-folder requests", async () => {
    let resolve!: (value: WorkspaceDirectory) => void;
    const fetch = vi.fn(
      () =>
        new Promise<WorkspaceDirectory>((done) => {
          resolve = done;
        }),
    );
    const { loader } = setup(fetch);
    const a = loader.load("w", 1, "folder", true),
      b = loader.load("w", 1, "folder");
    resolve({ revision_id: "r", path: "folder", entries: [], next_cursor: null });
    await Promise.all([a, b]);
    expect(fetch).toHaveBeenCalledTimes(1);
  });
  it("prefetches one child page and promotes it when selected", async () => {
    let childCalls = 0;
    const fetch = vi.fn(
      (
        _w: string,
        _v: number,
        path = "",
        cursor: string | null = null,
      ): Promise<WorkspaceDirectory> => {
        if (!path) {
          return Promise.resolve({
            revision_id: "r",
            path,
            entries: [directory("child")],
            next_cursor: null,
          });
        }

        childCalls++;

        return Promise.resolve({
          revision_id: "r",
          path,
          entries: [entry(childCalls)],
          next_cursor: cursor === null ? "next" : null,
        });
      },
    );
    const { loader, state } = setup(fetch);

    await loader.load("w", 1, "");

    await vi.waitFor(() => expect(state().child?.loading).toBe(false));
    expect(childCalls).toBe(1);
    expect(state().child?.complete).toBe(false);

    await loader.load("w", 1, "child");
    expect(childCalls).toBe(2);
    expect(state().child?.complete).toBe(true);
    expect(state().child?.entries).toHaveLength(2);
  });
  it("limits concurrency across resets and discards obsolete pages", async () => {
    const callbacks: (() => void)[] = [];
    let active = 0,
      maximum = 0;
    const fetch = vi.fn(
      (_w: string, _v: number, path = "") =>
        new Promise<WorkspaceDirectory>((resolve) => {
          active++;
          maximum = Math.max(active, maximum);
          callbacks.push(() => {
            active--;
            resolve({ revision_id: "r", path, entries: [entry(0)], next_cursor: null });
          });
        }),
    );
    const { loader, state } = setup(fetch);
    const requests = Array.from({ length: 8 }, (_, index) =>
      loader.load("w", 1, String(index), true),
    );
    loader.reset();
    const c = loader.load("w", 2, "c");

    expect(fetch).toHaveBeenCalledTimes(8);

    for (let index = 0; index < 8; index++) {
      callbacks.shift()?.();
    }

    await Promise.all(requests);
    callbacks.shift()?.();
    await c;
    expect(maximum).toBe(8);
    expect(Object.keys(state())).toEqual(["c"]);
  });
});
