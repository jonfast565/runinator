import type * as Api from "../commandCenterApi";
export { defaultApi } from "./default";

export type AuthApi = Pick<
  typeof Api,
  "fetchAuthConfig" | "fetchAuthMe" | "login" | "logout" | "refreshSession" | "setAccessToken"
>;
