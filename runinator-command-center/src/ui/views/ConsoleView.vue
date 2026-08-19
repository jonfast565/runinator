<template>
  <section class="pane h-full overflow-hidden">
    <SplitPane
      class="h-full w-full"
      storage-key="command-center.console.split"
      :initial-first-pct="74"
      :min-first="420"
      :min-second="240"
      collapsible-second
      mobile-mode="toggle"
      :mobile-detail-active="false"
    >
      <template #first>
        <div class="panel overflow-hidden p-0">
          <div class="panel-toolbar m-0 px-3 pt-3">
            <div class="flex items-center gap-2">
              <h2 class="m-0 text-base font-semibold text-fg">Console</h2>
              <code v-if="notebook.activeSession" class="text-[11px] text-fg-muted">{{
                notebook.activeSession.name
              }}</code>
            </div>
            <div class="btn-row">
              <select
                v-if="notebook.sessions.length"
                :value="notebook.activeSession?.id ?? ''"
                aria-label="Console session"
                @change="openSession(($event.target as HTMLSelectElement).value)"
              >
                <option v-for="session in notebook.sessions" :key="session.id" :value="session.id">
                  {{ session.name }}
                </option>
              </select>
              <button class="btn" :disabled="!canUse" @click="newSession">
                <Icon name="plus" />
                <span>New</span>
              </button>
              <button class="btn" :disabled="!canUse" @click="terminal.clear">
                <Icon name="trash" />
                <span>Clear</span>
              </button>
            </div>
          </div>

          <EmptyState
            v-if="!canUse"
            icon="lock"
            title="Console unavailable"
            description="Using the console requires the console:use action. A line can start a workflow run, so it is a privilege rather than a view."
          />
          <!-- clicking anywhere on the surface puts the caret back in the prompt, the way a
               terminal emulator does. -->
          <div
            v-else
            class="terminal-surface flex min-h-0 flex-1 flex-col"
            @click="focusPrompt"
          >
            <TerminalStatusBar
              :session="notebook.activeSession?.name ?? 'no session'"
              :busy="terminal.busy"
            />
            <TerminalTranscript :entries="terminal.entries" :cells="notebook.cells" />
            <TerminalPrompt
              ref="prompt"
              :busy="terminal.busy"
              :history="terminal.history"
              @submit="submit"
              @stop="terminal.stop"
              @clear="terminal.clear"
            />
          </div>
        </div>
      </template>

      <template #second>
        <div class="panel details overflow-auto">
          <h2 class="m-0 text-base font-semibold text-fg">Scope</h2>
          <p class="hint m-0">
            What <code>{{ CELL_SCOPE }}.&lt;name&gt;</code> resolves to in this session. A failed
            line drops its binding rather than leaving a stale value behind.
          </p>
          <EmptyState
            v-if="!notebook.bindings.length"
            compact
            icon="list"
            title="Empty scope"
            description="Run a line to bind its result to a name."
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
                <tr v-for="binding in notebook.bindings" :key="binding.id">
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
import SplitPane from "../components/shared/SplitPane.vue";
import TerminalPrompt from "../components/console/TerminalPrompt.vue";
import TerminalStatusBar from "../components/console/TerminalStatusBar.vue";
import TerminalTranscript from "../components/console/TerminalTranscript.vue";
import { useConsoleStore } from "../adapters/pinia/console";
import { useConsoleTerminalStore } from "../adapters/pinia/console-terminal";
import { useOrgsStore } from "../adapters/pinia/orgs";
import { useCan } from "../composables/useCan";
import { CELL_SCOPE } from "../../core/domain/models";
import type { JsonValue } from "../../core/domain/json";

// not named `console`: vue's template compiler treats that identifier as the global one, so
// `console.sessions` in a template would silently read `window.console`.
const notebook = useConsoleStore();
const terminal = useConsoleTerminalStore();
const orgs = useOrgsStore();
const { can } = useCan();
// the backend enforces this; the empty state exists so the reason is visible rather than the page
// simply failing to load.
const canUse = computed(() => can("console:use"));

const prompt = ref<InstanceType<typeof TerminalPrompt> | null>(null);

// one line of a bound value, so the scope panel stays scannable.
function preview(value: JsonValue): string {
  const text = typeof value === "string" ? value : JSON.stringify(value);
  return text.length > 80 ? `${text.slice(0, 80)}…` : text;
}

async function submit(line: string) {
  await terminal.submit(line);
}

async function openSession(sessionId: string) {
  await notebook.openSession(sessionId);
}

async function newSession() {
  await notebook.newSession();
}

async function refresh() {
  notebook.clearConsole();
  // the transcript belongs to the session it was typed against, so switching orgs starts a new one
  // rather than leaving output above a scope that no longer exists.
  terminal.reset();

  if (!canUse.value) {
    return;
  }

  // focused before the session load rather than after it: the prompt is already on screen, and
  // whether the tab is typeable should not depend on how long that call takes or whether it fails.
  await focusPrompt();
  await notebook.refreshSessions();
}

// a click that was really a text selection leaves the selection alone.
async function focusPrompt(event?: MouseEvent) {
  if (event && !(window.getSelection()?.isCollapsed ?? true)) {
    return;
  }

  await prompt.value?.focus();
}

onMounted(refresh);
watch(() => orgs.activeOrgId, refresh);
</script>
