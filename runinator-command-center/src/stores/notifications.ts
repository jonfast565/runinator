import { defineStore } from "pinia";
import { computed, ref } from "vue";
import {
  fetchNotifications,
  markAllNotificationsRead,
  markNotificationRead
} from "../api/commandCenterApi";
import type { Notification } from "../types/models";
import { useAppStore } from "./app";

export const useNotificationsStore = defineStore("notifications", () => {
  const app = useAppStore();
  const notifications = ref<Notification[]>([]);
  const unreadOnly = ref(false);

  const unreadCount = computed(() => notifications.value.filter((notification) => !notification.read_at).length);

  async function refreshNotifications() {
    notifications.value = await app
      .runOperation("Loading notifications", () => fetchNotifications({ unreadOnly: unreadOnly.value }))
      .catch(() => []);
  }

  function clearNotifications() {
    notifications.value = [];
  }

  async function markRead(id: string) {
    await app.runOperation("Marking notification read", () => markNotificationRead(id)).catch((error) => {
      app.setError(String(error));
    });
    await refreshNotifications();
  }

  async function markAllRead() {
    await app.runOperation("Marking all notifications read", () => markAllNotificationsRead()).catch((error) => {
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
    markAllRead
  };
});
