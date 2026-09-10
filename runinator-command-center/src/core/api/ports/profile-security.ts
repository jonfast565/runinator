import type * as Api from "../commandCenterApi";
export { defaultApi } from "./default";

export type ProfileSecurityApi = Pick<
  typeof Api,
  | "changeCurrentPassword"
  | "createPersonalApiKey"
  | "listCurrentSessions"
  | "listPersonalApiKeys"
  | "listPersonalApiKeyScopes"
  | "revokeApiKey"
  | "revokeCurrentSession"
  | "revokeOtherSessions"
  | "rotateApiKey"
  | "updateApiKey"
  | "updateCurrentUser"
>;
