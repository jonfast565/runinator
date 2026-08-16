<template>
  <section class="pane h-full overflow-hidden">
    <SplitPane
      class="h-full w-full"
      storage-key="command-center.console.split"
      :initial-first-pct="70"
      :min-first="440"
      :min-second="280"
      collapsible-second
      mobile-mode="toggle"
      :mobile-detail-active="false"
    >
      <template #first>
        <div class="panel overflow-auto">
          <div class="panel-toolbar">
            <h2 class="m-0 text-base font-semibold text-fg">Console</h2>
            <div class="btn-row">
              <select
                v-if="console.sessions.length"
                :value="console.activeSession?.id ?? ''"
                @change="openSession(($event.target as HTMLSelectElement).value)"
              >
                <option v-for="session in console.sessions" :key="session.id" :value="session.id">
                  {{ session.name }}
                </option>
              </select>
              <button class="btn" :disabled="!canUse" @click="newSession">
                <Icon name="plus" />
                <span>New</span>
              </button>
              <button class="btn" :disabled="loading" @click="console.refreshSessions">
                <LoadingSpinner v-if="loading" size="sm" label="Refreshing console" />
                <Icon v-else name="refresh" />
                <span>Refresh</span>
              </button>
              <button
                class="btn btn-danger"
                :disabled="!console.activeSession"
                @click="removeSession"
              >
                <Icon name="trash" />
                <span>Delete session</span>
              </button>
            </div>
          </div>

          <EmptyState
            v-if="!canUse"
            icon="lock"
            title="Console unavailable"
            description="Using the console requires the console:use capability. A cell can start a workflow run, so it is a privilege rather than a view."
          />
          <EmptyState
            v-else-if="!console.activeSession"
            icon="debug"
            title="No console session"
            description="A console session is a notebook of cells sharing one scope. Create one to start."
          />
          <template v-else>
            <p class="hint m-0">
              A cell is WDL. A pure expression is evaluated immediately; anything effectful runs as a
              workflow. A cell's result binds to
              <code>{{ CELL_SCOPE }}.&lt;name&gt;</code> for later cells.
            </p>

            <article
              v-for="cell in console.cells"
              :key="cell.id"
              class="panel gap-2 border border-border p-3"
              :class="{ danger: cell.status === 'failed', success: cell.status === 'succeeded' }"
            >
              <div class="panel-toolbar">
                <div class="flex items-center gap-2">
                  <StatusBadge :status="cell.status" />
                  <span v-if="cell.kind" class="hint m-0">{{ cellKindLabel(cell.kind) }}</span>
                  <code class="text-[11px] text-fg-muted">{{ cellReference(cell) }}</code>
                </div>
                <div class="btn-row">
                  <button
                    class="btn btn-sm btn-primary"
                    :disabled="console.isPending(cell.id)"
                    @click="runCell(cell)"
                  >
                    <LoadingSpinner
                      v-if="console.isPending(cell.id)"
                      size="sm"
                      label="Running cell"
                    />
                    <Icon v-else name="play" />
                    <span>Run</span>
                  </button>
                  <button class="btn btn-sm" @click="console.removeCell(cell.id)">
                    <Icon name="trash" />
                  </button>
                </div>
              </div>

              <textarea
                class="font-mono text-[12px]"
                rows="3"
                :value="cell.source"
                @change="saveCell(cell, ($event.target as HTMLTextAreaElement).value)"
              ></textarea>

              <pre v-if="cell.error" class="output danger">{{ cell.error }}</pre>
              <pre v-else-if="cell.result !== null && cell.result !== undefined" class="output">{{
                pretty(cell.result)
              }}</pre>
              <p v-if="cell.workflow_run_id" class="hint m-0">
                Ran as workflow
                <code class="text-[11px]">{{ cell.workflow_run_id }}</code>
              </p>
            </article>

            <div class="panel gap-2 border border-dashed border-border p-3">
              <label class="grid gap-1 text-xs text-fg-muted">
                New cell
                <textarea
                  v-model="draft"
                  class="font-mono text-[12px]"
                  rows="3"
                  placeholder="1 + 2"
                ></textarea>
              </label>
              <div class="btn-row items-end">
                <label class="grid gap-1 text-xs text-fg-muted">
                  Label (optional)
                  <input v-model="draftLabel" placeholder="total" />
                </label>
                <button class="btn btn-primary" :disabled="!draft.trim()" @click="addCell">
                  <Icon name="plus" />
                  <span>Add cell</span>
                </button>
              </div>
            </div>
          </template>
        </div>
      </template>

      <template #second>
        <div class="panel details overflow-auto">
          <h2 class="m-0 text-base font-semibold text-fg">Scope</h2>
          <p class="hint m-0">
            What <code>{{ CELL_SCOPE }}.&lt;name&gt;</code> resolves to in this session. A failed
            cell drops its binding rather than leaving a stale value behind.
          </p>
          <EmptyState
            v-if="!console.bindings.length"
            compact
            icon="list"
            title="Empty scope"
            description="Run a cell to bind its result to a name."
          />
          <DataTable v-else>
            <table>
              <thead>
                <tr>
                  <th>Name</th>
                  <th>Value</th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="binding in console.bindings" :key="binding.id">
                  <td class="font-mono text-[12px]">{{ binding.name }}</td>
                  <td class="font-mono text-[11px]">{{ preview(binding.value) }}</td>
                </tr>
              </tbody>
            </table>
          </DataTable>
        </div>
      </template>
    </SplitPane>
  </section>
</template>

<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import DataTable from "../components/shared/DataTable.vue";
import EmptyState from "../components/shared/EmptyState.vue";
import Icon from "../components/shared/Icon.vue";
import LoadingSpinner from "../components/shared/LoadingSpinner.vue";
import SplitPane from "../components/shared/SplitPane.vue";
import StatusBadge from "../components/shared/StatusBadge.vue";
import { useConsoleStore } from "../adapters/pinia/console";
import { useOrgsStore } from "../adapters/pinia/orgs";
import { useOperationLoading } from "../composables/useOperationLoading";
import { useCan } from "../composables/useCan";
import { pretty } from "../../core/utils/format";
import { CELL_SCOPE, cellReference } from "../../core/domain/models";
import type { ConsoleCell, ConsoleCellKind } from "../../core/domain/models";
import type { JsonValue } from "../../core/domain/json";

const console = useConsoleStore();
const orgs = useOrgsStore();
const { isLoading: loading } = useOperationLoading("Refreshing console sessions");
const { can } = useCan();
// the backend enforces this; the empty state exists so the reason is visible rather than the page
// simply failing to load.
const canUse = computed(() => can("console:use"));

const draft = ref("");
const draftLabel = ref("");

// says why a cell did or did not start a run, using what the backend classified it as rather than
// re-deriving it from source that may since have been edited.
function cellKindLabel(kind: ConsoleCellKind): string {
  return kind === "workflow" ? "ran as a workflow" : `evaluated as ${kind}`;
}

// one line of a bound value, so the scope panel stays scannable.
function preview(value: JsonValue): string {
  const text = typeof value === "string" ? value : JSON.stringify(value);
  return text.length > 80 ? `${text.slice(0, 80)}…` : text;
}

async function addCell() {
  await console.addCell(draft.value, draftLabel.value.trim() || null);
  draft.value = "";
  draftLabel.value = "";
}

// saved on change rather than on every keystroke: editing a cell clears its previous result on the
// backend, and doing that per character would make the result flicker away as someone types.
async function saveCell(cell: ConsoleCell, source: string) {
  if (source === cell.source) {
    return;
  }

  await console.editCell(cell.id, source, cell.label ?? null);
}

async function runCell(cell: ConsoleCell) {
  await console.runCell(cell.id);
}

async function openSession(sessionId: string) {
  await console.openSession(sessionId);
}

async function newSession() {
  await console.newSession();
}

async function removeSession() {
  const sessionId = console.activeSession?.id;

  if (sessionId) {
    await console.removeSession(sessionId);
  }
}

async function refresh() {
  console.clearConsole();

  if (canUse.value) {
    await console.refreshSessions();
  }
}

onMounted(refresh);
watch(() => orgs.activeOrgId, refresh);
</script>
