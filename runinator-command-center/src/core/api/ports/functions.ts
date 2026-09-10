import type * as Api from "../commandCenterApi";
export { defaultApi } from "./default";

export type FunctionsApi = Pick<
  typeof Api,
  | "deleteFunctionAlias"
  | "deleteFunctionPackage"
  | "fetchFunctionCatalog"
  | "fetchFunctionPackage"
  | "fetchFunctionPackages"
  | "publishFunctionVersion"
  | "restoreFunctionPackage"
  | "setFunctionAlias"
  | "uploadFunctionArtifact"
>;
