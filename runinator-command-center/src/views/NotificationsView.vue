<template>
  <section class="pane notifications-pane">
    <div class="panel">
      <div class="panel-toolbar">
        <h2>Notifications</h2>
        <div class="btn-row">
          <label class="filter-toggle">
            <input type="checkbox" v-model="store.unreadOnly" @change="refresh" />
            <span>Unread only</span>
          </label>
          <button class="btn" :disabled="loading" @click="refresh">
            <Icon name="refresh" />
            <span>Refresh</span>
          </button>
          <button class="btn" :disabled="loading || store.unreadCount === 0" @click="markAllRead">
            <Icon name="check" />
            <span>Mark all read</span>
          </button>
        </div>
      </div>
      <DataTable>
        <table>
          <thead>
            <tr>
              <th>ID</th>
              <th>Channel</th>
              <th>Severity</th>
              <th>Title</th>
              <th>Body</th>
              <th>Run</th>
              <th>Created</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            <tr
              v-for="notification in store.notifications"
              :key="notification.id"
              :class="{ unread: !notification.read_at, danger: notification.severity === 'error', success: notification.severity === 'success', warning: notification.severity === 'warning' }"
            >
              <td>{{ notification.id }}</td>
              <td>{{ notification.channel }}</td>
              <td>{{ notification.severity }}</td>
              <td>{{ notification.title }}</td>
              <td class="body-cell">{{ notification.body ?? "" }}</td>
              <td>{{ notification.workflow_run_id ?? "" }}</td>
              <td>{{ formatDate(notification.created_at) }}</td>
              <td class="row-actions">
                <button v-if="!notification.read_at" class="btn btn-icon btn-ghost" title="Mark read" @click="markRead(notification.id)">
                  <Icon name="check" />
                </button>
              </td>
            </tr>
          </tbody>
        </table>
      </DataTable>
    </div>
  </section>
</template>

<script setup lang="ts">
import { onMounted, ref } from "vue";
import DataTable from "../components/shared/DataTable.vue";
import Icon from "../components/shared/Icon.vue";
import { useNotificationsStore } from "../stores/notifications";
import { formatDate } from "../utils/format";

const store = useNotificationsStore();
const loading = ref(false);

async function refresh() {
  loading.value = true;
  try {
    await store.refreshNotifications();
  } finally {
    loading.value = false;
  }
}

async function markRead(id: number) {
  await store.markRead(id);
}

async function markAllRead() {
  await store.markAllRead();
}

onMounted(refresh);
</script>

<style scoped>
.body-cell {
  max-width: 380px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.row-actions {
  text-align: right;
}

tr.unread td:nth-child(4) {
  font-weight: 700;
}

.filter-toggle {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  color: #475260;
  font-size: 12px;
}

.filter-toggle input {
  width: auto;
}
</style>
