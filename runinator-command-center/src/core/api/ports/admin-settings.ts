import type * as Api from "../commandCenterApi";
export { defaultApi } from "./default";

export type AdminSettingsApi = Pick<
  typeof Api,
  | "fetchCredentials"
  | "fetchForeignLanguageRuntime"
  | "fetchAuthSettings"
  | "fetchServerSettings"
  | "saveForeignLanguageRuntime"
  | "saveAuthSettings"
  | "saveServerSettings"
>;
