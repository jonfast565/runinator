import type * as Api from "../commandCenterApi";
export { defaultApi } from "./default";

export type CatalogMetadataApi = Pick<
  typeof Api,
  "fetchEnumCatalogs" | "fetchNodeKinds" | "fetchTriggerKinds"
>;
