import { defaultApi, type ConsoleWorkspacesApi } from "../../api/ports/console-workspaces";
// execution adapters only; parsing, usage and completion come from the shared clap catalog.

import { json, text } from "../format";
import { flag, flagSet, numberFlag, requiredArg } from "../options";
import type { ConsoleCommand } from "../types";
import { UnavailableCommandError } from "../types";
import { ctlCatalog } from "../wasm-engine";

export function createWorkspacesCommands(api: ConsoleWorkspacesApi = defaultApi) {
  const handlers: Record<string, ConsoleCommand["run"]> = {
    list: async ({ flags, print }) => {
      print(json(await api.fetchDurableWorkspaces(numberFlag(flags, "offset") ?? 0)));
    },
    create: async ({ args, print }) => {
      print(json(await api.createDurableWorkspace(requiredArg(args, 0, "key"))));
    },
    versions: async ({ args, flags, print }) => {
      print(
        json(
          await api.fetchWorkspaceVersions(
            requiredArg(args, 0, "workspace"),
            numberFlag(flags, "offset") ?? 0,
          ),
        ),
      );
    },
    ls: async ({ args, flags, print }) => {
      print(
        json(
          await api.fetchWorkspaceDirectory(
            requiredArg(args, 0, "workspace"),
            Number(requiredArg(args, 1, "version")),
            args[2] ?? "",
            flag(flags, "cursor") ?? null,
            flagSet(flags, "results"),
          ),
        ),
      );
    },
    cat: async ({ args, flags, print }) => {
      print(
        text(
          await api.previewWorkspaceFile(
            requiredArg(args, 0, "workspace"),
            Number(requiredArg(args, 1, "version")),
            requiredArg(args, 2, "path"),
            flagSet(flags, "result"),
          ),
        ),
      );
    },
    diff: async ({ args, flags, print }) => {
      print(
        json(
          await api.fetchWorkspaceDiff(
            requiredArg(args, 0, "workspace"),
            Number(requiredArg(args, 1, "before")),
            Number(requiredArg(args, 2, "after")),
            flag(flags, "cursor") ?? null,
          ),
        ),
      );
    },
    export: async ({ args, flags, print }) => {
      print(
        json(
          await api.createWorkspaceTransfer(
            requiredArg(args, 0, "workspace"),
            Number(requiredArg(args, 1, "version")),
            false,
            flagSet(flags, "filesystem"),
          ),
        ),
      );
    },
    job: async ({ args, print }) => {
      print(json(await api.fetchWorkspaceTransfer(requiredArg(args, 0, "id"))));
    },
    cancel: async ({ args, print }) => {
      await api.cancelWorkspaceTransfer(requiredArg(args, 0, "id"));
      print(text("Transfer cancelled"));
    },
    import: () => {
      throw new UnavailableCommandError(
        "workspaces import",
        "use the Workspaces view to select an archive, or run the native CLI",
      );
    },
    download: () => {
      throw new UnavailableCommandError(
        "workspaces download",
        "use the Workspaces view to select a destination, or run the native CLI",
      );
    },
  };
  const workspaceCommands: ConsoleCommand[] = Object.entries(handlers).map(([name, run]) => ({
    path: ["workspaces", name],
    get usage() {
      return (
        ctlCatalog().find((entry) => entry.path.join(" ") === `workspaces ${name}`)?.usage ?? ""
      );
    },
    get summary() {
      return (
        ctlCatalog().find((entry) => entry.path.join(" ") === `workspaces ${name}`)?.summary ?? ""
      );
    },
    run,
  }));

  return { workspaceCommands };
}

export const { workspaceCommands } = createWorkspacesCommands();
