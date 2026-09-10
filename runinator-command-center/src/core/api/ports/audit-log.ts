import type * as Api from "../commandCenterApi";
export { defaultApi } from "./default";

export type AuditLogApi = Pick<typeof Api, "listAuditLog">;
