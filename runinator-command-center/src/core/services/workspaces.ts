import {
  fetchDurableWorkspaces,
  fetchWorkspaceVersions,
  deleteDurableWorkspace,
  downloadWorkspaceVersion,
} from "../api/commandCenterApi";
import type { DurableWorkspace, WorkspaceSnapshot } from "../domain/models/workspaces";
import { createStore } from "./event-bus";

export function createWorkspacesService() {
  const store = createStore({
    items: [] as DurableWorkspace[],
    selected: null as DurableWorkspace | null,
    versions: [] as WorkspaceSnapshot[],
  });
  let generation = 0;
  return {
    ...store,
    async refresh(offset = 0) {
      const token = ++generation;
      const items = await fetchDurableWorkspaces(offset);

      if (token === generation) {
        store.setState((current) => ({ ...current, items }));
      }
    },
    async select(selected: DurableWorkspace | null, offset = 0) {
      const token = ++generation;
      store.setState((current) => ({ ...current, selected, versions: [] }));

      if (!selected) {
        return;
      }

      const versions = await fetchWorkspaceVersions(selected.id, offset);

      if (token === generation) {
        store.setState((current) => ({ ...current, versions }));
      }
    },
    async remove(id: string, version: number | null = null) {
      await deleteDurableWorkspace(id, version);
    },
    download: downloadWorkspaceVersion,
    clear() {
      generation++;
      store.setState(() => ({ items: [], selected: null, versions: [] }));
    },
  };
}
