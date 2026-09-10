import type * as Api from "../commandCenterApi";
export { defaultApi } from "./default";

export type WorkflowsEditorApi = Pick<typeof Api, "compileRexRap" | "decompileToRexRap">;
