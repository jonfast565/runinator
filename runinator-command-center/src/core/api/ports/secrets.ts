import type * as Api from "../commandCenterApi";
export { defaultApi } from "./default";

export type SecretsApi = Pick<
  typeof Api,
  "deleteCredential" | "fetchCredential" | "fetchCredentials" | "moveCredential" | "saveCredential"
>;
