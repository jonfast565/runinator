import type * as Api from "../commandCenterApi";
export { defaultApi } from "./default";
export type ConsoleSessionsApi = Pick<
  typeof Api,
  | "cancelConsoleCell"
  | "createConsoleCell"
  | "createConsoleSession"
  | "clearConsoleSession"
  | "deleteConsoleCell"
  | "deleteConsoleSession"
  | "fetchConsoleCell"
  | "fetchConsoleSession"
  | "fetchConsoleSessions"
  | "renameConsoleSession"
  | "replayConsoleCell"
  | "runConsoleCell"
  | "updateConsoleCell"
>;
