import type * as Api from "../commandCenterApi";
export { defaultApi } from "./default";

export type ProvidersApi = Pick<typeof Api, "fetchProviders">;
