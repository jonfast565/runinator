// execution adapters only; parsing, usage and completion come from the shared clap catalog.
import {
  fetchDurableWorkspaces,
  createDurableWorkspace,
  fetchWorkspaceVersions,
  fetchWorkspaceDirectory,
  fetchWorkspaceDiff,
  previewWorkspaceFile,
  createWorkspaceTransfer,
  fetchWorkspaceTransfer,
  cancelWorkspaceTransfer,
} from "../../api/commandCenterApi";
import { json, text } from "../format";
import { flag, flagSet, numberFlag, requiredArg } from "../options";
import type { ConsoleCommand } from "../types";
import { UnavailableCommandError } from "../types";
import { ctlCatalog } from "../wasm-engine";

const handlers: Record<string, ConsoleCommand["run"]> = {
  list: async ({ flags, print }) => {
    print(json(await fetchDurableWorkspaces(numberFlag(flags, "offset") ?? 0)));
  },
  create: async ({ args, print }) => {
    print(json(await createDurableWorkspace(requiredArg(args, 0, "key"))));
  },
  versions: async ({ args, flags, print }) => {
    print(
      json(
        await fetchWorkspaceVersions(
          requiredArg(args, 0, "workspace"),
          numberFlag(flags, "offset") ?? 0,
        ),
      ),
    );
  },
  ls: async ({ args, flags, print }) => {
    print(
      json(
        await fetchWorkspaceDirectory(
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
        await previewWorkspaceFile(
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
        await fetchWorkspaceDiff(
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
        await createWorkspaceTransfer(
          requiredArg(args, 0, "workspace"),
          Number(requiredArg(args, 1, "version")),
          false,
          flagSet(flags, "filesystem"),
        ),
      ),
    );
  },
  job: async ({ args, print }) => {
    print(json(await fetchWorkspaceTransfer(requiredArg(args, 0, "id"))));
  },
  cancel: async ({ args, print }) => {
    await cancelWorkspaceTransfer(requiredArg(args, 0, "id"));
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
export const workspaceCommands: ConsoleCommand[] = Object.entries(handlers).map(([name, run]) => ({
  path: ["workspaces", name],
  get usage() {
    return ctlCatalog().find((entry) => entry.path.join(" ") === `workspaces ${name}`)?.usage ?? "";
  },
  get summary() {
    return (
      ctlCatalog().find((entry) => entry.path.join(" ") === `workspaces ${name}`)?.summary ?? ""
    );
  },
  run,
}));
