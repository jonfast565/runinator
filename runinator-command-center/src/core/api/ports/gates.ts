import type * as Api from "../commandCenterApi";
export { defaultApi } from "./default";

export type GatesApi = Pick<typeof Api, "closeGate" | "deleteGate" | "fetchGates" | "openGate">;
