import type * as Api from "../commandCenterApi";
export { defaultApi } from "./default";

export type ConsoleSettingsApi = Pick<
  typeof Api,
  "deleteCredential" | "fetchCredential" | "fetchCredentials" | "saveCredential"
>;
