import type * as Api from "../commandCenterApi";
export { defaultApi } from "./default";

export type ConsoleFunctionsApi = Pick<
  typeof Api,
  | "deleteFunctionAlias"
  | "deleteFunctionPackage"
  | "fetchFunctionCatalog"
  | "fetchFunctionPackage"
  | "fetchFunctionPackages"
  | "invokeFunction"
  | "restoreFunctionPackage"
  | "setFunctionAlias"
>;
