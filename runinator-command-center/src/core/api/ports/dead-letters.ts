import type * as Api from "../commandCenterApi";
export { defaultApi } from "./default";

export type DeadLettersApi = Pick<typeof Api, "listDeadLetters">;
