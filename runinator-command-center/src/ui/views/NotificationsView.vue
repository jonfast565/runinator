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

    <div v-if="canManagePolicies" class="panel">
      <PanelHeader title="Alert policies">
        <Button variant="default" :loading="policiesLoading" @click="refreshPolicies">
          <Icon name="refresh" />
          <span>Refresh</span>
        </Button>
        <button class="btn" :disabled="policiesLoading" @click="startCreate">
          <Icon name="plus" />
          <span>New policy</span>
        </button>
      </PanelHeader>

      <form v-if="draft" class="border-b border-border px-4 py-3" @submit.prevent="savePolicy">
        <div class="grid gap-3 sm:grid-cols-2 lg:grid-cols-4 [&_label]:flex [&_label]:flex-col [&_label]:gap-1 [&_label]:text-xs [&_label]:text-fg-muted">
          <label>
            <span>Name</span>
            <input v-model="draft.name" required placeholder="oncall" />
          </label>
          <label>
            <span>Event</span>
            <select v-model="draft.event">
              <option v-for="event in events" :key="event" :value="event">
                {{ eventLabel(event) }}
              </option>
            </select>
          </label>
          <label>
            <span>Channel</span>
            <select v-model="draft.channel">
              <option value="in_app">In-app</option>
              <option value="slack">Slack</option>
              <option value="email">Email</option>
            </select>
          </label>
          <label>
            <span>Severity</span>
            <select v-model="draft.severity">
              <option value="info">Info</option>
              <option value="warning">Warning</option>
              <option value="critical">Critical</option>
            </select>
          </label>
          <label>
            <span>Target</span>
            <input
              v-model="draftTarget"
              :required="draft.channel !== 'in_app'"
              :placeholder="draft.channel === 'email' ? 'ops@example.com' : '#oncall'"
            />
          </label>
          <label v-if="needsThreshold">
            <span>After (minutes)</span>
            <input v-model.number="thresholdMinutes" type="number" min="1" required />
          </label>
          <label class="!flex-row items-center gap-2 self-end [&>input]:w-auto">
            <input v-model="draft.enabled" type="checkbox" />
            <span>Enabled</span>
          </label>
        </div>
        <div class="mt-3 flex gap-2">
          <Button variant="primary" type="submit" :loading="policiesLoading">
            <span>{{ editingId ? "Save policy" : "Create policy" }}</span>
          </Button>
          <button class="btn" type="button" @click="cancelEdit">Cancel</button>
        </div>
      </form>

      <DataTable
        :columns="policyColumns"
        :rows="store.policies"
        row-key="id"
        :page-size="10"
        :loading="policiesLoading"
        loading-message="Loading policies…"
        responsive="cards"
        empty-icon="bell"
        empty-title="No alert policies"
        empty-description="Without a policy, a failed run raises no alert. Add one here, or declare a notify line in the workflow's REXRAP."
      >
        <template #cell-event="{ row }">{{ eventLabel(row.event) }}</template>
        <template #cell-workflow_id="{ row }">{{ row.workflow_id ? "scoped" : "global" }}</template>
        <template #cell-threshold_seconds="{ row }">{{
          row.threshold_seconds ? `${Math.round(row.threshold_seconds / 60)}m` : "—"
        }}</template>
        <template #cell-managed_by="{ row }">{{ row.managed_by ?? "ui" }}</template>
        <template #cell-enabled="{ row }">{{ row.enabled ? "yes" : "no" }}</template>
        <template #cell-actions="{ row }">
          <span class="text-right">
            <!-- pack-managed rows are reconciled on the next import, so editing them here would be
                 silently reverted; point the operator at the .rexrap instead. -->
            <button
              class="btn btn-icon btn-ghost"
              :disabled="Boolean(row.managed_by)"
              :title="row.managed_by ? 'Managed by the pack — edit the .rexrap' : 'Edit'"
              @click.stop="startEdit(row)"
            >
              <Icon name="edit" />
            </button>
            <button
              class="btn btn-icon btn-ghost"
              :disabled="Boolean(row.managed_by)"
              :title="row.managed_by ? 'Managed by the pack — remove the notify line' : 'Delete'"
              @click.stop="removePolicy(row.id)"
            >
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
import { useActionsStore } from "../../ui/adapters/pinia/actions";
import type {
  NewNotificationPolicy,
  Notification,
  NotificationEvent,
  NotificationPolicy,
} from "../../core/domain/models";
import { DURATION_NOTIFICATION_EVENTS } from "../../core/domain/models";
import { formatDate } from "../../core/utils/format";

const store = useNotificationsStore();
const app = useAppStore();
const actions = useActionsStore();
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

// ---- alert policies ----

const canManagePolicies = computed(() => actions.has("notifications:manage"));
const policiesLoading = ref(false);
const draft = ref<NewNotificationPolicy | null>(null);
const editingId = ref<string | null>(null);

const events: NotificationEvent[] = [
  "run_failed",
  "node_retry_exhausted",
  "run_sla_breached",
  "run_parked",
];

const EVENT_LABELS: Record<NotificationEvent, string> = {
  run_failed: "Run failed",
  node_retry_exhausted: "Retries exhausted",
  run_sla_breached: "SLA breached",
  run_parked: "Parked too long",
};

function eventLabel(event: NotificationEvent): string {
  return EVENT_LABELS[event];
}

const policyColumns: DataTableColumn<NotificationPolicy>[] = [
  { key: "name", label: "Name", sortable: true },
  { key: "event", label: "Event", sortable: true },
  { key: "channel", label: "Channel", sortable: true },
  { key: "target", label: "Target" },
  { key: "severity", label: "Severity", sortable: true },
  { key: "threshold_seconds", label: "After" },
  { key: "workflow_id", label: "Scope" },
  { key: "managed_by", label: "Source" },
  { key: "enabled", label: "Enabled", sortable: true },
  { key: "actions", label: "", align: "right" },
];

const needsThreshold = computed(
  () => !!draft.value && DURATION_NOTIFICATION_EVENTS.includes(draft.value.event),
);

// the wire field is nullable but the input binds a plain string; bridge the two.
const draftTarget = computed({
  get: () => draft.value?.target ?? "",
  set: (value: string) => {
    if (draft.value) {
      draft.value.target = value;
    }
  },
});

// operators think in minutes; the contract is seconds.
const thresholdMinutes = computed({
  get: () => (draft.value?.threshold_seconds ? draft.value.threshold_seconds / 60 : 30),
  set: (value: number) => {
    if (draft.value) {
      draft.value.threshold_seconds = Math.max(1, Math.round(value)) * 60;
    }
  },
});

function startCreate() {
  editingId.value = null;
  draft.value = {
    workflow_id: null,
    name: "",
    event: "run_failed",
    severity: "warning",
    channel: "slack",
    target: "",
    threshold_seconds: null,
    enabled: true,
    managed_by: null,
    configuration: null,
  };
}

function startEdit(policy: NotificationPolicy) {
  editingId.value = policy.id;
  draft.value = {
    workflow_id: policy.workflow_id ?? null,
    name: policy.name,
    event: policy.event,
    severity: policy.severity,
    channel: policy.channel,
    target: policy.target ?? "",
    threshold_seconds: policy.threshold_seconds ?? null,
    enabled: policy.enabled,
    managed_by: policy.managed_by ?? null,
    configuration: policy.configuration ?? null,
  };
}

function cancelEdit() {
  draft.value = null;
  editingId.value = null;
}

async function refreshPolicies() {
  policiesLoading.value = true;

  try {
    await store.refreshPolicies();
  } finally {
    policiesLoading.value = false;
  }
}

async function savePolicy() {
  if (!draft.value) {
    return;
  }

  const payload: NewNotificationPolicy = {
    ...draft.value,
    target: draft.value.target?.trim() ? draft.value.target.trim() : null,
    // a transition event carries no threshold; clear any value left behind by switching the event.
    threshold_seconds: needsThreshold.value ? (draft.value.threshold_seconds ?? 1800) : null,
  };

  policiesLoading.value = true;

  try {
    if (await store.savePolicy(payload, editingId.value ?? undefined)) {
      cancelEdit();
    }
  } finally {
    policiesLoading.value = false;
  }
}

async function removePolicy(policyId: string) {
  policiesLoading.value = true;

  try {
    await store.removePolicy(policyId);
  } finally {
    policiesLoading.value = false;
  }
}

onMounted(async () => {
  await refresh();

  if (canManagePolicies.value) {
    await refreshPolicies();
  }
});
</script>

