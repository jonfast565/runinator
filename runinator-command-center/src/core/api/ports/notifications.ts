import type * as Api from "../commandCenterApi";
export { defaultApi } from "./default";

export type NotificationsApi = Pick<
  typeof Api,
  | "applyNotificationAction"
  | "createNotificationPolicy"
  | "deleteNotification"
  | "deleteNotificationPolicy"
  | "fetchNotificationPolicies"
  | "fetchNotifications"
  | "markAllNotificationsRead"
  | "markNotificationRead"
  | "updateNotificationPolicy"
>;
