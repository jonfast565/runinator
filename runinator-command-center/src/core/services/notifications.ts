import {
  deleteNotification,
  fetchNotifications,
  markAllNotificationsRead,
  markNotificationRead,
} from "../api/commandCenterApi";
import type { Notification } from "../domain/models";
import { createStore } from "./event-bus";
import type { AppService } from "./app";

export interface NotificationsState {
  notifications: Notification[];
  unreadOnly: boolean;
}

export function createNotificationsService(app: AppService) {
  const store = createStore<NotificationsState>({
    notifications: [],
    unreadOnly: false,
  });

  function unreadCount(): number {
    return store.getState().notifications.filter((notification) => !notification.read_at).length;
  }

  const service = {
    ...store,
    unreadCount,
    setUnreadOnly(value: boolean) {
      store.setState((state) => ({ ...state, unreadOnly: value }));
    },
    async refreshNotifications() {
      const { unreadOnly } = store.getState();
      const notifications = await app
        .runOperation("Loading notifications", () => fetchNotifications({ unreadOnly }))
        .catch(() => []);
      store.setState((state) => ({ ...state, notifications }));
    },
    clearNotifications() {
      store.setState((state) => ({ ...state, notifications: [] }));
    },
    async markRead(id: string) {
      await app
        .runOperation("Marking notification read", () => markNotificationRead(id))
        .catch((error: unknown) => {
          app.setError(String(error));
        });
      await service.refreshNotifications();
    },
    async markAllRead() {
      await app
        .runOperation("Marking all notifications read", () => markAllNotificationsRead())
        .catch((error: unknown) => {
          app.setError(String(error));
        });
      await service.refreshNotifications();
    },
    async remove(id: string) {
      await app
        .runOperation("Deleting notification", () => deleteNotification(id))
        .catch((error: unknown) => {
          app.setError(String(error));
        });
      store.setState((state) => ({
        ...state,
        notifications: state.notifications.filter((notification) => notification.id !== id),
      }));
      await service.refreshNotifications();
    },
    async removeAllRead() {
      const readIds = store
        .getState()
        .notifications.filter((notification) => notification.read_at)
        .map((notification) => notification.id);

      if (!readIds.length) {
        return;
      }

      await app
        .runOperation("Deleting read notifications", async () => {
          for (const notificationId of readIds) {
            await deleteNotification(notificationId);
          }
        })
        .catch((error: unknown) => {
          app.setError(String(error));
        });
      await service.refreshNotifications();
    },
  };

  return service;
}

export type NotificationsService = ReturnType<typeof createNotificationsService>;
