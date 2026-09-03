import {
  createNotificationPolicy,
  deleteNotification,
  deleteNotificationPolicy,
  fetchNotificationPolicies,
  fetchNotifications,
  markAllNotificationsRead,
  markNotificationRead,
  updateNotificationPolicy,
} from "../api/commandCenterApi";
import type {
  NewNotificationPolicy,
  Notification,
  NotificationPolicy,
} from "../domain/models";
import { createStore } from "./event-bus";
import type { AppService } from "./app";

export interface NotificationsState {
  notifications: Notification[];
  unreadOnly: boolean;
  policies: NotificationPolicy[];
}

export function createNotificationsService(app: AppService) {
  const store = createStore<NotificationsState>({
    notifications: [],
    unreadOnly: false,
    policies: [],
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
        .runOperation("Loading notifications", () => fetchNotifications({ unreadOnly }), {
          retryable: true,
        })
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
        .runOperation("Dismissing notification", () => deleteNotification(id))
        .catch((error: unknown) => {
          app.setError(String(error));
        });
      store.setState((state) => ({
        ...state,
        notifications: state.notifications.filter((notification) => notification.id !== id),
      }));
      await service.refreshNotifications();
    },
    async refreshPolicies() {
      const policies = await app
        .runOperation("Loading notification policies", () => fetchNotificationPolicies(), {
          retryable: true,
        })
        .catch(() => []);
      store.setState((state) => ({ ...state, policies }));
    },
    async savePolicy(policy: NewNotificationPolicy, policyId?: string) {
      // a validation rejection from the backend is the useful signal here (an unroutable channel or
      // a threshold-less duration event), so surface it rather than silently swallowing it.
      try {
        await app.runOperation("Saving notification policy", () =>
          policyId
            ? updateNotificationPolicy(policyId, policy)
            : createNotificationPolicy(policy),
        );
      } catch (error: unknown) {
        app.setError(String(error));
        return false;
      }

      await service.refreshPolicies();

      return true;
    },
    async removePolicy(policyId: string) {
      try {
        await app.runOperation("Deleting notification policy", () =>
          deleteNotificationPolicy(policyId),
        );
      } catch (error: unknown) {
        app.setError(String(error));
        return false;
      }

      await service.refreshPolicies();

      return true;
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
        .runOperation("Dismissing read notifications", async () => {
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
