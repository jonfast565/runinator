import { defineStore } from "pinia";
import { computed, ref } from "vue";
import {
  deleteNotification,
  fetchNotifications,
  markAllNotificationsRead,
  markNotificationRead,
} from "../api/commandCenterApi";
import type { Notification } from "../types/models";
import { useAppStore } from "./app";

export const useNotificationsStore = defineStore("notifications", () => {
  const app = useAppStore();
  const notifications = ref<Notification[]>([]);
  const unreadOnly = ref(false);

  const unreadCount = computed(
    () => notifications.value.filter((notification) => !notification.read_at).length,
  );

  async function refreshNotifications() {
    notifications.value = await app
      .runOperation("Loading notifications", () =>
        fetchNotifications({ unreadOnly: unreadOnly.value }),
      )
      .catch(() => []);
  }

  function clearNotifications() {
    notifications.value = [];
  }

  async function markRead(id: string) {
    await app
      .runOperation("Marking notification read", () => markNotificationRead(id))
      .catch((error: unknown) => {
        app.setError(String(error));
      });
    await refreshNotifications();
  }

  async function markAllRead() {
    await app
      .runOperation("Marking all notifications read", () => markAllNotificationsRead())
      .catch((error: unknown) => {
        app.setError(String(error));
      });
    await refreshNotifications();
  }

  async function remove(id: string) {
    await app
      .runOperation("Deleting notification", () => deleteNotification(id))
      .catch((error: unknown) => {
        app.setError(String(error));
      });
    // drop locally for immediate feedback, then reconcile with the server.
    notifications.value = notifications.value.filter((notification) => notification.id !== id);
    await refreshNotifications();
  }

  async function removeAllRead() {
    const readIds = notifications.value
      .filter((notification) => notification.read_at)
      .map((n) => n.id);

    if (!readIds.length) {
      return;
    }

    await app
      .runOperation("Deleting read notifications", async () => {
        for (const id of readIds) {
          await deleteNotification(id);
        }
      })
      .catch((error: unknown) => {
        app.setError(String(error));
      });
    await refreshNotifications();
  }

  return {
    notifications,
    unreadOnly,
    unreadCount,
    refreshNotifications,
    clearNotifications,
    markRead,
    markAllRead,
    remove,
    removeAllRead,
  };
});
