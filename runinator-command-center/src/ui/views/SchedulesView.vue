<template>
  <section class="pane">
    <div class="panel">
      <PanelHeader title="Freeze windows">
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

      <p class="border-b border-border px-4 py-2 text-xs text-fg-muted">
        While a window is in effect its triggers are held, not dropped: the due slot survives the
        freeze and each trigger's catch-up policy decides what happens when the window lifts.
      </p>

      <form v-if="draft" class="border-b border-border px-4 py-3" @submit.prevent="save">
        <div
          class="grid gap-3 sm:grid-cols-2 lg:grid-cols-4 [&_label]:flex [&_label]:flex-col [&_label]:gap-1 [&_label]:text-xs [&_label]:text-fg-muted"
        >
          <label>
            <span>Name</span>
            <input v-model="draft.name" required placeholder="December change freeze" />
          </label>
          <label>
            <span>Starts</span>
            <input v-model="startsAtLocal" type="datetime-local" required />
          </label>
          <label>
            <span>Ends</span>
            <input v-model="endsAtLocal" type="datetime-local" required />
          </label>
          <label>
            <span>Workflow ID (blank freezes all)</span>
            <input v-model="draftWorkflowId" placeholder="all workflows" />
          </label>
          <label class="sm:col-span-2">
            <span>Reason</span>
            <input v-model="draftReason" placeholder="holiday change freeze" />
          </label>
          <label class="!flex-row items-center gap-2 self-end [&>input]:w-auto">
            <input v-model="draft.enabled" type="checkbox" />
            <span>Enabled</span>
          </label>
        </div>
        <div class="mt-3 flex gap-2">
          <Button variant="primary" type="submit" :loading="loading">
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
        <template #cell-starts_at="{ row }">{{ formatDate(row.starts_at) }}</template>
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
import PanelHeader from "../components/shared/PanelHeader.vue";
import { useSchedulesStore } from "../../ui/adapters/pinia/schedules";
import { useAppStore } from "../../ui/adapters/pinia/app";
import { useCapabilitiesStore } from "../../ui/adapters/pinia/capabilities";
import type { FreezeWindow, NewFreezeWindow } from "../../core/domain/models";
import { formatDate } from "../../core/utils/format";

const store = useSchedulesStore();
const app = useAppStore();
const capabilities = useCapabilitiesStore();
const loading = ref(false);
const draft = ref<NewFreezeWindow | null>(null);
const editingId = ref<string | null>(null);

const canManage = computed(() => capabilities.has("schedules:manage"));

const columns: DataTableColumn<FreezeWindow>[] = [
  { key: "name", label: "Name", sortable: true },
  { key: "scope", label: "Scope" },
  { key: "starts_at", label: "Starts", sortable: true },
  { key: "ends_at", label: "Ends", sortable: true },
  { key: "state", label: "State" },
  { key: "reason", label: "Reason" },
  { key: "actions", label: "", align: "right" },
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

// the datetime-local control speaks local wall time; the wire is utc.
function toLocalInput(iso: string): string {
  const date = new Date(iso);
  const offset = date.getTimezoneOffset() * 60_000;

  return new Date(date.getTime() - offset).toISOString().slice(0, 16);
}

function fromLocalInput(value: string): string {
  return new Date(value).toISOString();
}

const startsAtLocal = computed({
  get: () => (draft.value ? toLocalInput(draft.value.starts_at) : ""),
  set: (value: string) => {
    if (draft.value && value) {
      draft.value.starts_at = fromLocalInput(value);
    }
  },
});

const endsAtLocal = computed({
  get: () => (draft.value ? toLocalInput(draft.value.ends_at) : ""),
  set: (value: string) => {
    if (draft.value && value) {
      draft.value.ends_at = fromLocalInput(value);
    }
  },
});

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
  if (!draft.value) {
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
