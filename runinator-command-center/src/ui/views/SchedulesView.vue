<template>
  <section class="pane">
    <div class="panel">
      <PanelHeader
        title="Calendar sync"
        icon="calendar"
        eyebrow="Outlook and iCalendar"
        description="Export visible workflow and pipeline runtimes, or create a private subscription URL that Outlook refreshes automatically."
      />

      <div class="grid gap-4 lg:grid-cols-[minmax(0,0.7fr)_minmax(0,1.3fr)]">
        <div class="rounded-md border border-border bg-surface-soft p-4">
          <label class="flex flex-col gap-1 text-xs text-fg-muted">
            <span>Calendar scope</span>
            <select v-model="calendarScope">
              <option value="user">Everything I can view</option>
              <option value="organization" :disabled="!orgs.activeOrgId">
                Active organization
              </option>
              <option value="platform">Platform</option>
            </select>
          </label>
          <p class="mb-3 mt-2 text-xs text-fg-muted">
            Organization and platform feeds require matching access. Resource grants are rechecked
            whenever a subscribed calendar refreshes.
          </p>
          <div class="flex flex-wrap gap-2">
            <Button variant="default" :loading="calendarBusy" @click="downloadCalendar">
              <Icon name="download" />
              <span>Download .ics</span>
            </Button>
            <Button variant="primary" :loading="calendarBusy" @click="createSubscription">
              <Icon name="link" />
              <span>Create sync URL</span>
            </Button>
          </div>
        </div>

        <div class="rounded-md border border-border bg-surface-soft p-4">
          <template v-if="calendarUrl">
            <div class="flex items-center justify-between gap-3">
              <div>
                <p class="m-0 text-sm font-semibold">Private calendar URL</p>
                <p class="mb-2 mt-1 text-xs text-fg-muted">
                  In Outlook, choose Add calendar → Subscribe from web. Treat this URL like a secret.
                </p>
              </div>
              <button class="btn btn-icon btn-ghost" title="Copy URL" @click="copyCalendarUrl">
                <Icon name="copy" />
              </button>
            </div>
            <input :value="calendarUrl" readonly aria-label="Calendar subscription URL" />
            <div class="mt-2 flex items-center justify-between gap-3">
              <span class="text-xs text-success">{{ calendarCopyStatus }}</span>
              <button class="btn btn-ghost text-danger" :disabled="calendarBusy" @click="revokeSubscription">
                Revoke URL
              </button>
            </div>
          </template>
          <div v-else class="flex min-h-24 items-center justify-center text-center text-sm text-fg-muted">
            Create a revocable feed URL for Outlook, Apple Calendar, or any RFC 5545 client.
          </div>
        </div>
      </div>
    </div>

    <div class="panel">
      <PanelHeader
        title="Freeze windows"
        icon="clock"
        eyebrow="Operational safety"
        description="Freeze only the scope you intend. The end must be later than the start; times are entered locally and stored in UTC."
      >
        <label class="inline-flex items-center gap-1.5 text-xs text-fg-muted">
          <input v-model="store.activeOnly" type="checkbox" class="w-auto" @change="refresh" />
          <span>Active only</span>
        </label>
        <Button variant="default" :loading="loading" @click="refresh">
          <Icon name="refresh" />
          <span>Refresh</span>
        </Button>
        <button v-if="canManage" class="btn" :disabled="loading" @click="startCreate">
          <Icon name="plus" />
          <span>New window</span>
        </button>
      </PanelHeader>

      <div class="grid grid-cols-1 gap-2 sm:grid-cols-3">
        <MetricCard label="Visible windows" :value="filteredWindows.length" />
        <MetricCard label="Active now" :value="activeWindowCount" />
        <MetricCard label="Upcoming" :value="upcomingWindowCount" />
      </div>

      <form
        v-if="draft"
        class="rounded-md border border-accent/25 bg-accent-soft px-4 py-3"
        @submit.prevent="save"
      >
        <div
          class="grid gap-3 sm:grid-cols-2 lg:grid-cols-4 [&_label]:flex [&_label]:flex-col [&_label]:gap-1 [&_label]:text-xs [&_label]:text-fg-muted"
        >
          <label>
            <span>Name</span>
            <input
              v-model.trim="draft.name"
              required
              minlength="2"
              maxlength="100"
              placeholder="December change freeze"
            />
          </label>
          <label>
            <span>Workflow ID (blank freezes all)</span>
            <input v-model="draftWorkflowId" data-validation="uuid" placeholder="all workflows" />
          </label>
          <label class="sm:col-span-2">
            <span>Reason</span>
            <input v-model="draftReason" maxlength="500" placeholder="holiday change freeze" />
          </label>
          <label class="!flex-row items-center gap-2 self-end [&>input]:w-auto">
            <input v-model="draft.enabled" type="checkbox" />
            <span>Enabled</span>
          </label>
        </div>
        <ScheduleEditor
          v-if="draft.schedule"
          v-model="draft.schedule"
          class="mt-3"
          window
          title="Freeze recurrence"
          description="Use one shared calendar model for one-time, weekday, cron, or RRULE freeze windows."
        />
        <p v-if="scheduleError" class="error mb-0 mt-2 text-xs" role="alert">
          {{ scheduleError }}
        </p>
        <div class="mt-3 flex gap-2">
          <Button
            variant="primary"
            type="submit"
            :loading="loading"
            :disabled="Boolean(scheduleError)"
          >
            <span>{{ editingId ? "Save window" : "Create window" }}</span>
          </Button>
          <button class="btn" type="button" @click="cancelEdit">Cancel</button>
        </div>
      </form>

      <DataTable
        :columns="columns"
        :rows="filteredWindows"
        row-key="id"
        :page-size="25"
        :loading="loading"
        loading-message="Loading freeze windows…"
        responsive="cards"
        initial-sort-key="starts_at"
        initial-sort-dir="desc"
        empty-icon="clock"
        :empty-title="store.freezeWindows.length ? 'No matches' : 'No freeze windows'"
        empty-description="A freeze window suspends cron trigger firing over a date range, for a workflow, an org, or the whole platform."
      >
        <template #cell-scope="{ row }">{{ scopeLabel(row) }}</template>
        <template #cell-starts_at="{ row }">{{ row.schedule ? describeSchedule(row.schedule) : formatDate(row.starts_at) }}</template>
        <template #cell-ends_at="{ row }">{{ formatDate(row.ends_at) }}</template>
        <template #cell-state="{ row }">{{ stateLabel(row) }}</template>
        <template #cell-actions="{ row }">
          <span class="text-right">
            <button
              class="btn btn-icon btn-ghost"
              :disabled="!canManage"
              title="Edit"
              @click.stop="startEdit(row)"
            >
              <Icon name="edit" />
            </button>
            <button
              class="btn btn-icon btn-ghost"
              :disabled="!canManage"
              title="Delete"
              @click.stop="remove(row.id)"
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
import MetricCard from "../components/shared/MetricCard.vue";
import PanelHeader from "../components/shared/PanelHeader.vue";
import ScheduleEditor from "../components/shared/ScheduleEditor.vue";
import { useSchedulesStore } from "../../ui/adapters/pinia/schedules";
import { useAppStore } from "../../ui/adapters/pinia/app";
import { useActionsStore } from "../../ui/adapters/pinia/actions";
import { useOrgsStore } from "../../ui/adapters/pinia/orgs";
import type { CalendarScope, FreezeWindow, NewFreezeWindow } from "../../core/domain/models";
import {
  calendarSubscriptionUrl,
  createCalendarSubscription,
  deleteCalendarSubscription,
  downloadScheduleCalendar,
} from "../../core/api/commandCenterApi";
import { downloadBlob } from "../adapters/browser/files";
import { formatDate } from "../../core/utils/format";
import { defaultSchedule, describeSchedule, validateSchedule } from "../../core/workflow/schedule";

const store = useSchedulesStore();
const app = useAppStore();
const actions = useActionsStore();
const orgs = useOrgsStore();
const loading = ref(false);
const draft = ref<NewFreezeWindow | null>(null);
const editingId = ref<string | null>(null);
const calendarScope = ref<CalendarScope>("user");
const calendarBusy = ref(false);
const calendarUrl = ref("");
const calendarSubscriptionId = ref("");
const calendarCopyStatus = ref("");

const canManage = computed(() => actions.has("schedules:manage"));

function calendarOrgId(): string | null {
  return calendarScope.value === "organization" ? orgs.activeOrgId : null;
}

async function downloadCalendar() {
  calendarBusy.value = true;

  try {
    const blob = await downloadScheduleCalendar(calendarScope.value, calendarOrgId());
    downloadBlob("runinator-schedules.ics", blob);
  } catch (error) {
    app.setError(String(error));
  } finally {
    calendarBusy.value = false;
  }
}

async function createSubscription() {
  calendarBusy.value = true;
  calendarCopyStatus.value = "";

  try {
    const secret = await createCalendarSubscription(calendarScope.value, calendarOrgId());
    calendarSubscriptionId.value = secret.subscription.id;
    calendarUrl.value = calendarSubscriptionUrl(secret.token, app.serviceUrl);
  } catch (error) {
    app.setError(String(error));
  } finally {
    calendarBusy.value = false;
  }
}

async function copyCalendarUrl() {
  try {
    await navigator.clipboard.writeText(calendarUrl.value);
    calendarCopyStatus.value = "Copied";
  } catch {
    calendarCopyStatus.value = "Select the URL and copy it manually.";
  }
}

async function revokeSubscription() {
  if (!calendarSubscriptionId.value) {return;}
  calendarBusy.value = true;

  try {
    await deleteCalendarSubscription(calendarSubscriptionId.value);
    calendarUrl.value = "";
    calendarSubscriptionId.value = "";
    calendarCopyStatus.value = "";
  } catch (error) {
    app.setError(String(error));
  } finally {
    calendarBusy.value = false;
  }
}

const columns: DataTableColumn<FreezeWindow>[] = [
  { key: "name", label: "Name", sortable: true, mobile: true },
  { key: "scope", label: "Scope" },
  { key: "starts_at", label: "Schedule", sortable: true },
  { key: "ends_at", label: "Current / next end", sortable: true },
  { key: "state", label: "State" },
  { key: "reason", label: "Reason" },
  { key: "actions", label: "", align: "right", mobile: true },
];

function scopeLabel(window: FreezeWindow): string {
  if (window.workflow_id) {
    return "one workflow";
  }

  return window.org_id ? "one org" : "platform";
}

// "in effect right now" is the question an operator actually has when a schedule looks stuck.
function stateLabel(window: FreezeWindow): string {
  if (!window.enabled) {
    return "disabled";
  }

  const now = Date.now();

  if (Date.parse(window.starts_at) > now) {
    return "upcoming";
  }

  return Date.parse(window.ends_at) > now ? "active" : "expired";
}

const filteredWindows = computed(() => {
  const query = app.normalizedSearch;

  if (!query) {
    return store.freezeWindows;
  }

  return store.freezeWindows.filter((window) =>
    [window.name, window.reason ?? "", window.workflow_id ?? ""].some((value) =>
      value.toLowerCase().includes(query),
    ),
  );
});
const activeWindowCount = computed(
  () => filteredWindows.value.filter((window) => stateLabel(window) === "active").length,
);
const upcomingWindowCount = computed(
  () => filteredWindows.value.filter((window) => stateLabel(window) === "upcoming").length,
);

const draftWorkflowId = computed({
  get: () => draft.value?.workflow_id ?? "",
  set: (value: string) => {
    if (draft.value) {
      draft.value.workflow_id = value.trim() ? value.trim() : null;
    }
  },
});

const draftReason = computed({
  get: () => draft.value?.reason ?? "",
  set: (value: string) => {
    if (draft.value) {
      draft.value.reason = value.trim() ? value : null;
    }
  },
});

const scheduleError = computed(() => {
  if (!draft.value) {
    return "";
  }

  return draft.value.schedule
    ? validateSchedule(draft.value.schedule, true)
    : "Choose a freeze recurrence.";
});

function startCreate() {
  const now = new Date();
  const tomorrow = new Date(now.getTime() + 24 * 60 * 60 * 1000);
  editingId.value = null;
  draft.value = {
    org_id: null,
    workflow_id: null,
    name: "",
    reason: null,
    starts_at: now.toISOString(),
    ends_at: tomorrow.toISOString(),
    schedule: defaultSchedule(true),
    enabled: true,
  };
}

function startEdit(window: FreezeWindow) {
  editingId.value = window.id;
  draft.value = {
    org_id: window.org_id ?? null,
    workflow_id: window.workflow_id ?? null,
    name: window.name,
    reason: window.reason ?? null,
    starts_at: window.starts_at,
    ends_at: window.ends_at,
    schedule: window.schedule ?? {
      recurrence: { kind: "once", at: window.starts_at },
      timezone: "UTC",
      duration_seconds: Math.max(1, Math.round((Date.parse(window.ends_at) - Date.parse(window.starts_at)) / 1000)),
    },
    enabled: window.enabled,
  };
}

function cancelEdit() {
  draft.value = null;
  editingId.value = null;
}

async function refresh() {
  loading.value = true;

  try {
    await store.refreshFreezeWindows();
  } finally {
    loading.value = false;
  }
}

async function save() {
  if (!draft.value || scheduleError.value) {
    return;
  }

  loading.value = true;

  try {
    const saved = await store.saveFreezeWindow(draft.value, editingId.value ?? undefined);

    if (saved) {
      cancelEdit();
    }
  } finally {
    loading.value = false;
  }
}

async function remove(windowId: string) {
  const freezeWindow = store.freezeWindows.find((candidate) => candidate.id === windowId);

  if (!freezeWindow || !window.confirm(`Delete freeze window “${freezeWindow.name}”?`)) {
    return;
  }

  loading.value = true;

  try {
    await store.removeFreezeWindow(windowId);
  } finally {
    loading.value = false;
  }
}

onMounted(() => {
  void refresh();
});
</script>
