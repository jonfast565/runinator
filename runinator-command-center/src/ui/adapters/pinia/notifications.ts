import { defineStore } from "pinia";
import { computed } from "vue";
import { notificationsService } from "../../../core/services";
import { mirrorServiceState } from "./sync";

export const useNotificationsStore = defineStore("notifications", () => {
  const state = mirrorServiceState(notificationsService);

  return {
    notifications: computed(() => state.value.notifications),
    unreadOnly: computed({
      get: () => state.value.unreadOnly,
      set: (value) => { notificationsService.setUnreadOnly(value); },
    }),
    unreadCount: computed(() => notificationsService.unreadCount()),
    refreshNotifications: () => notificationsService.refreshNotifications(),
    clearNotifications: () => { notificationsService.clearNotifications(); },
    markRead: (id: string) => notificationsService.markRead(id),
    markAllRead: () => notificationsService.markAllRead(),
    remove: (id: string) => notificationsService.remove(id),
    removeAllRead: () => notificationsService.removeAllRead(),
  };
});
