import type * as Api from "../commandCenterApi";
export { defaultApi } from "./default";

export type WorkspacesApi = Pick<
  typeof Api,
  | "fetchDurableWorkspaces"
  | "fetchWorkspaceVersions"
  | "fetchWorkspaceSnapshot"
  | "deleteDurableWorkspace"
  | "downloadWorkspaceVersion"
  | "fetchWorkspaceDirectory"
  | "fetchWorkspaceDiff"
  | "createWorkspaceTransfer"
  | "fetchWorkspaceTransfer"
  | "cancelWorkspaceTransfer"
  | "importWorkspaceArchive"
  | "previewWorkspaceFile"
>;
