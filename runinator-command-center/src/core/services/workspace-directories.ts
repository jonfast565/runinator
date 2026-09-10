import type { WorkspaceDirectory } from "../domain/models/workspaces";
import type { WorkspacesApi } from "../api/ports/workspaces";

export interface DirectoryState extends WorkspaceDirectory {
  loading: boolean;
  complete: boolean;
  error: string;
}

export function emptyDirectories(): Partial<Record<string, DirectoryState>> {
  return Object.create(null) as Partial<Record<string, DirectoryState>>;
}

// one scheduler survives resets so stale in-flight requests still count toward the limit.
export function createDirectoryLoader(
  api: WorkspacesApi,
  publish: (directories: Partial<Record<string, DirectoryState>>, selected: string) => void,
) {
  let generation = 0;
  let directories: Partial<Record<string, DirectoryState>> = emptyDirectories();
  let selected = "";
  let revision = "";
  let active = 0;
  const expanded = new Set<string>();
  const pending = new Map<string, Promise<void>>();
  const queue: { path: string; generation: number; run: () => void }[] = [];

  const emit = () => {
    publish(Object.assign(emptyDirectories(), directories), selected);
  };

  const wanted = (path: string) => path === selected || expanded.has(path);

  function pump() {
    queue.sort((a, b) => Number(b.path === selected) - Number(a.path === selected));

    while (active < 2 && queue.length) {
      queue.shift()?.run();
    }
  }

  function reset(nextRevision = "") {
    generation++;
    directories = emptyDirectories();
    selected = "";
    revision = nextRevision;
    expanded.clear();
    pending.clear();
    emit();
    pump();
  }

  async function load(workspace: string, version: number, path: string, tree = false) {
    if (tree) {
      expanded.add(path);
    } else {
      selected = path;
    }

    const existing = pending.get(path);

    if (existing) {
      emit();
      pump();
      return existing;
    }

    if (directories[path]?.complete) {
      emit();
      return;
    }

    const token = generation;
    directories[path] = {
      ...(directories[path] ?? {
        revision_id: revision,
        path,
        entries: [],
        next_cursor: null,
        complete: false,
      }),
      loading: true,
      error: "",
    };
    emit();
    const work = new Promise<void>((resolve) => {
      const finish = () => {
        if (token === generation && directories[path]) {
          pending.delete(path);
          directories[path] = { ...directories[path], loading: false };
          emit();
        }

        resolve();
      };

      const enqueue = () => {
        queue.push({
          path,
          generation: token,
          run: () => {
            if (token !== generation || !wanted(path)) {
              finish();
              return;
            }

            const current = directories[path];

            if (!current) {
              finish();
              return;
            }

            active++;
            void api
              .fetchWorkspaceDirectory(workspace, version, path, current.next_cursor, false)
              .then((page) => {
                if (token !== generation) {
                  return;
                }

                if ((revision && page.revision_id !== revision) || page.path !== path) {
                  throw new Error("Directory response belongs to another revision or path.");
                }

                revision = page.revision_id;

                if (page.next_cursor && page.next_cursor === current.next_cursor) {
                  throw new Error("Directory cursor did not advance.");
                }

                const entries = new Map(current.entries.map((entry) => [entry.name, entry]));

                for (const entry of page.entries) {
                  entries.set(entry.name, entry);
                }

                directories[path] = {
                  ...page,
                  entries: [...entries.values()],
                  complete: !page.next_cursor,
                  loading: true,
                  error: "",
                };
                emit();
              })
              .catch((reason: unknown) => {
                if (token !== generation) {
                  return;
                }

                directories[path] = {
                  ...current,
                  error: reason instanceof Error ? reason.message : String(reason),
                };
              })
              .finally(() => {
                active--;

                if (
                  token === generation &&
                  wanted(path) &&
                  !directories[path]?.complete &&
                  !directories[path]?.error
                ) {
                  enqueue();
                } else {
                  finish();
                }

                pump();
              });
          },
        });
      };

      enqueue();
    });
    pending.set(path, work);
    pump();
    return work;
  }

  return {
    load,
    reset,
    collapse(path: string) {
      for (const folder of expanded) {
        if (!path || folder === path || folder.startsWith(`${path}/`)) {
          expanded.delete(folder);
        }
      }
    },
  };
}
