import {
  fetchDurableWorkspaces,
  fetchWorkspaceVersions,
  fetchWorkspaceSnapshot,
  deleteDurableWorkspace,
  downloadWorkspaceVersion,
  fetchWorkspaceDirectory,
  fetchWorkspaceDiff,
  createWorkspaceTransfer,
  fetchWorkspaceTransfer,
  cancelWorkspaceTransfer,
  importWorkspaceArchive,
  previewWorkspaceFile,
} from "../api/commandCenterApi";
import type {
  DurableWorkspace,
  WorkspaceSnapshot,
  WorkspaceDirectory,
  WorkspaceDiff,
} from "../domain/models/workspaces";
import { createStore } from "./event-bus";

export function createWorkspacesService() {
  const store = createStore({
    items: [] as DurableWorkspace[],
    selected: null as DurableWorkspace | null,
    versions: [] as WorkspaceSnapshot[],
    pinnedSnapshot: null as WorkspaceSnapshot | null,
    directory: null as WorkspaceDirectory | null,
    results: null as WorkspaceDirectory | null,
    preview: "",
    diff: null as WorkspaceDiff | null,
    transfer: null as import("../domain/models/workspaces").WorkspaceTransfer | null,
  });
  let listGeneration = 0;
  let detailGeneration = 0;
  let directoryGeneration = 0;
  let resultsGeneration = 0;
  let previewGeneration = 0;
  let diffGeneration = 0;
  let activeVersion = "";

  function activate(workspace: string, version: number) {
    const next = `${workspace}:${String(version)}`;

    if (next === activeVersion) {
      return;
    }

    activeVersion = next;
    directoryGeneration++;
    resultsGeneration++;
    previewGeneration++;
    diffGeneration++;
    store.setState((current) => ({
      ...current,
      directory: null,
      results: null,
      preview: "",
      diff: null,
    }));
  }

  return {
    ...store,
    async refresh(offset = 0) {
      const token = ++listGeneration;
      const items = await fetchDurableWorkspaces(offset).catch((error: unknown) => {
        if (token === listGeneration) {
          throw error;
        }

        return null;
      });

      if (items === null) {
        return;
      }

      if (token === listGeneration) {
        store.setState((current) => ({ ...current, items }));
      }
    },
    async select(selected: DurableWorkspace | null, offset = 0, pinned: number | null = null) {
      activeVersion = "";
      const token = ++detailGeneration;
      directoryGeneration++;
      diffGeneration++;
      resultsGeneration++;
      previewGeneration++;
      store.setState((current) => ({
        ...current,
        selected,
        versions: [],
        pinnedSnapshot: null,
        directory: null,
        results: null,
        preview: "",
        diff: null,
      }));

      if (!selected) {
        return;
      }

      const versions = await fetchWorkspaceVersions(selected.id, offset).catch((error: unknown) => {
        if (token === detailGeneration) {
          throw error;
        }

        return null;
      });

      if (versions === null) {
        return;
      }

      let pinnedSnapshot: WorkspaceSnapshot | null = null;

      if (pinned !== null && !versions.some((snapshot) => snapshot.version === pinned)) {
        pinnedSnapshot = await fetchWorkspaceSnapshot(selected.id, pinned).catch(
          (error: unknown) => {
            if (token === detailGeneration) {
              throw error;
            }

            return null;
          },
        );
      }

      if (token === detailGeneration) {
        store.setState((current) => ({ ...current, versions, pinnedSnapshot }));
      }
    },
    async browse(
      workspace: string,
      version: number,
      path = "",
      cursor: string | null = null,
      results = false,
    ) {
      activate(workspace, version);
      const token = results ? ++resultsGeneration : ++directoryGeneration;
      previewGeneration++;
      store.setState((current) => ({
        ...current,
        [results ? "results" : "directory"]: null,
        preview: "",
      }));
      const page = await fetchWorkspaceDirectory(workspace, version, path, cursor, results).catch(
        (error: unknown) => {
          if (token === (results ? resultsGeneration : directoryGeneration)) {
            throw error;
          }

          return null;
        },
      );

      if (page === null) {
        return;
      }

      if (token === (results ? resultsGeneration : directoryGeneration)) {
        store.setState((current) => ({ ...current, [results ? "results" : "directory"]: page }));
      }
    },
    async compare(workspace: string, before: number, after: number, cursor: string | null = null) {
      activate(workspace, after);
      const token = ++diffGeneration;
      store.setState((current) => ({ ...current, diff: null }));
      const diff = await fetchWorkspaceDiff(workspace, before, after, cursor).catch(
        (error: unknown) => {
          if (token === diffGeneration) {
            throw error;
          }

          return null;
        },
      );

      if (diff === null) {
        return;
      }

      if (token === diffGeneration) {
        store.setState((current) => ({ ...current, diff }));
      }
    },
    async previewFile(workspace: string, version: number, path: string, result = false) {
      activate(workspace, version);
      const token = ++previewGeneration;
      store.setState((current) => ({ ...current, preview: "" }));
      const preview = await previewWorkspaceFile(workspace, version, path, result).catch(
        (error: unknown) => {
          if (token === previewGeneration) {
            throw error;
          }

          return null;
        },
      );

      if (preview === null) {
        return;
      }

      if (token === previewGeneration) {
        store.setState((current) => ({ ...current, preview }));
      }
    },
    clearPreview() {
      previewGeneration++;
      store.setState((current) => ({ ...current, preview: "" }));
    },
    async remove(id: string, version: number | null = null) {
      await deleteDurableWorkspace(id, version);
    },
    async download(workspace: string, version: number, path: string | null = null, result = false) {
      if (path !== null) {
        return downloadWorkspaceVersion(workspace, version, path, result);
      }

      const generation = detailGeneration;
      let job = await createWorkspaceTransfer(workspace, version);

      while (generation === detailGeneration) {
        store.setState((current) => ({ ...current, transfer: job }));

        if (job.state === "ready") {
          return downloadWorkspaceVersion(workspace, version, null, false, job.id);
        }

        if (["failed", "cancelled"].includes(job.state)) {
          throw new Error(job.error ?? `Export ${job.state}`);
        }

        await new Promise((resolve) => setTimeout(resolve, 1500));
        job = await fetchWorkspaceTransfer(job.id);
      }

      return null;
    },
    async importArchive(key: string, file: File | null) {
      const generation = detailGeneration;
      let job = await importWorkspaceArchive(key, file);

      if (!job) {
        return;
      }

      while (generation === detailGeneration) {
        store.setState((current) => ({ ...current, transfer: job }));

        if (job.state === "ready") {
          return;
        }

        if (["failed", "cancelled"].includes(job.state)) {
          throw new Error(job.error ?? `Import ${job.state}`);
        }

        await new Promise((resolve) => setTimeout(resolve, 1500));
        job = await fetchWorkspaceTransfer(job.id);
      }
    },
    async cancelTransfer() {
      const job = store.getState().transfer;

      if (job) {
        await cancelWorkspaceTransfer(job.id);
      }
    },
    clear() {
      activeVersion = "";
      listGeneration++;
      detailGeneration++;
      directoryGeneration++;
      diffGeneration++;
      resultsGeneration++;
      previewGeneration++;
      store.setState(() => ({
        items: [],
        selected: null,
        versions: [],
        pinnedSnapshot: null,
        directory: null,
        results: null,
        preview: "",
        diff: null,
        transfer: null,
      }));
    },
  };
}
