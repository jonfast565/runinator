<template>
  <section class="pane">
    <div class="panel">
      <PanelHeader title="Notifications">
        <label class="inline-flex items-center gap-1.5 text-xs text-fg-muted">
          <input v-model="store.unreadOnly" type="checkbox" class="w-auto" @change="refresh" />
          <span>Unread only</span>
        </label>
        <Button variant="default" :loading="loading" @click="refresh">
          <Icon name="refresh" />
          <span>Refresh</span>
        </Button>
        <button class="btn" :disabled="loading || store.unreadCount === 0" @click="markAllRead">
          <Icon name="check" />
          <span>Mark all read</span>
        </button>
        <button class="btn" :disabled="loading || !hasRead" @click="deleteRead">
          <Icon name="trash" />
          <span>Delete read</span>
        </button>
      </PanelHeader>
      <DataTable
        :columns="columns"
        :rows="filteredNotifications"
        row-key="id"
        :page-size="25"
        :loading="loading"
        loading-message="Loading notifications…"
        responsive="cards"
        :row-class="rowClass"
        initial-sort-key="created_at"
        initial-sort-dir="desc"
        empty-icon="bell"
        :empty-title="store.notifications.length ? 'No matches' : 'No notifications yet'"
        :empty-description="
          store.notifications.length
            ? `No notifications match “${app.searchQuery}”.`
            : 'In-app and email notifications will appear here.'
        "
      >
        <template #cell-title="{ row }"
          ><span :class="{ 'font-bold': !row.read_at }">{{ row.title }}</span></template
        >
        <template #cell-body="{ row }"
          ><span class="max-w-[380px] truncate">{{ row.body ?? "" }}</span></template
        >
        <template #cell-created_at="{ row }">{{ formatDate(row.created_at) }}</template>
        <template #cell-actions="{ row }">
          <span class="text-right">
            <button
              v-if="!row.read_at"
              class="btn btn-icon btn-ghost"
              title="Mark read"
              @click.stop="markRead(row.id)"
            >
              <Icon name="check" />
            </button>
            <button class="btn btn-icon btn-ghost" title="Delete" @click.stop="remove(row.id)">
              <Icon name="trash" />
            </button>
          </span>
        </template>
      </DataTable>
    </div>
  </section>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import DataTable, { type DataTableColumn } from "../components/shared/DataTable.vue";
import Button from "../components/shared/Button.vue";
import Icon from "../components/shared/Icon.vue";
import PanelHeader from "../components/shared/PanelHeader.vue";
import { useNotificationsStore } from "../../ui/adapters/pinia/notifications";
import { useAppStore } from "../../ui/adapters/pinia/app";
import type { Notification } from "../../core/domain/models";
import { formatDate } from "../../core/utils/format";

const store = useNotificationsStore();
const app = useAppStore();
const loading = ref(false);

const columns: DataTableColumn<Notification>[] = [
  { key: "id", label: "ID", sortable: true },
  { key: "channel", label: "Channel", sortable: true },
  { key: "severity", label: "Severity", sortable: true },
  { key: "title", label: "Title", sortable: true },
  { key: "body", label: "Body" },
  { key: "workflow_run_id", label: "Run" },
  { key: "created_at", label: "Created", sortable: true },
  { key: "actions", label: "", align: "right" },
];

function rowClass(notification: Notification): Record<string, boolean> {
  return {
    unread: !notification.read_at,
    danger: notification.severity === "error",
    success: notification.severity === "success",
    warning: notification.severity === "warning",
  };
}

const hasRead = computed(() => store.notifications.some((notification) => notification.read_at));

// filter notifications by the global search box (matches title, body, channel, or severity).
const filteredNotifications = computed(() => {
  const query = app.normalizedSearch;

  if (!query) {
    return store.notifications;
  }

  return store.notifications.filter((notification) =>
    [
      notification.title,
      notification.body,
      notification.channel,
      notification.severity,
      notification.workflow_run_id ?? "",
    ].some((value) => (value ?? "").toLowerCase().includes(query)),
  );
});

async function refresh() {
  loading.value = true;

  try {
    await store.refreshNotifications();
  } finally {
    loading.value = false;
  }
}

async function markRead(id: string) {
  await store.markRead(id);
}

async function markAllRead() {
  await store.markAllRead();
}

async function remove(id: string) {
  await store.remove(id);
}

async function deleteRead() {
  await store.removeAllRead();
}

onMounted(refresh);
</script>
