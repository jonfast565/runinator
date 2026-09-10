import type * as Api from "../commandCenterApi";
export { defaultApi } from "./default";

export type ConsoleWorkspacesApi = Pick<
  typeof Api,
  | "fetchDurableWorkspaces"
  | "createDurableWorkspace"
  | "fetchWorkspaceVersions"
  | "fetchWorkspaceDirectory"
  | "fetchWorkspaceDiff"
  | "previewWorkspaceFile"
  | "createWorkspaceTransfer"
  | "fetchWorkspaceTransfer"
  | "cancelWorkspaceTransfer"
>;
