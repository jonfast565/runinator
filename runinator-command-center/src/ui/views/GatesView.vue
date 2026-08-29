<template>
  <section class="pane h-full overflow-hidden">
    <SplitPane
      class="h-full w-full"
      storage-key="command-center.gates.split"
      :initial-first-pct="58"
      :min-first="420"
      :min-second="340"
      collapsible-second
      mobile-mode="toggle"
      :mobile-detail-active="!!gates.selectedGate"
    >
      <template #first>
        <div class="panel">
          <PanelHeader
            title="Gates"
            description="Select a blocking gate and verify its run before changing the outcome."
          >
            <div class="btn-row">
              <button class="btn" :disabled="loadingGates" @click="gates.refreshGates">
                <LoadingSpinner v-if="loadingGates" size="sm" label="Refreshing gates" />
                <Icon v-else name="refresh" />
                <span>Refresh</span>
              </button>
              <button
                class="btn btn-primary"
                :disabled="!gates.canResolveSelected || !reason.trim()"
                @click="resolve('open')"
              >
                <Icon name="approve" />
                <span>Open</span>
              </button>
              <button
                class="btn btn-danger"
                :disabled="!gates.canResolveSelected || !reason.trim()"
                @click="resolve('close')"
              >
                <Icon name="reject" />
                <span>Close</span>
              </button>
              <button class="btn" :disabled="!gates.selectedGate" @click="removeSelected">
                <Icon name="trash" />
                <span>Delete</span>
              </button>
            </div>
          </PanelHeader>
          <DataTable>
            <thead>
              <tr>
                <th>Status</th>
                <th>Kind</th>
                <th>Label</th>
                <th>Node</th>
                <th>Run</th>
              </tr>
            </thead>
            <tbody>
              <tr v-if="loadingGates && !gates.gates.length">
                <td colspan="5" class="px-3.5 py-3.5 text-center text-fg-muted">
                  <LoadingPanel compact :message="loadingGatesMessage || 'Refreshing gates…'" />
                </td>
              </tr>
              <tr v-else-if="!gates.filteredGates.length">
                <td colspan="5" class="!p-0 hover:!bg-transparent">
                  <EmptyState
                    compact
                    :icon="gates.gates.length ? 'search' : 'lock'"
                    :title="gates.gates.length ? 'No matches' : 'No gates blocking'"
                    :description="
                      gates.gates.length
                        ? `No gates match “${app.searchQuery}”.`
                        : 'A gate holds a run until its condition passes or an operator opens it. None are blocking right now.'
                    "
                  />
                </td>
              </tr>
              <tr
                v-for="gate in gates.filteredGates"
                :key="String(gate.id ?? JSON.stringify(gate))"
                class="cursor-pointer"
                :class="{
                  selected: gates.selectedGate === gate,
                  danger: isBadStatus(gate.status),
                  success: isGoodStatus(gate.status),
                }"
                @click="gates.selectedGate = gate"
              >
                <td><StatusBadge :status="gate.status" /></td>
                <td>{{ gate.kind ?? "" }}</td>
                <td>{{ gate.label ?? "" }}</td>
                <td>{{ gate.node_id ?? "" }}</td>
                <td class="font-mono text-[11px]">{{ gate.workflow_run_id ?? "" }}</td>
              </tr>
            </tbody>
          </DataTable>
        </div>
      </template>
      <template #second>
        <div class="panel details overflow-hidden">
          <MobileBackBar @back="gates.selectedGate = null" />
          <div class="flex items-center gap-1">
            <h2 class="m-0 text-base font-semibold text-fg">Gate Detail</h2>
            <HelpBubble
              text="A reason is required so the operator decision remains auditable."
              label="About resolving gates"
            />
          </div>
          <label class="grid gap-1 text-xs text-fg-muted">
            Reason
            <input
              v-model.trim="reason"
              required
              maxlength="240"
              placeholder="Why are you opening or closing this gate?"
            />
          </label>
          <pre class="output">{{ gates.selectedGate ? pretty(gates.selectedGate) : "" }}</pre>
        </div>
      </template>
    </SplitPane>
  </section>
</template>

<script setup lang="ts">
import { onMounted, ref, watch } from "vue";
import DataTable from "../components/shared/DataTable.vue";
import EmptyState from "../components/shared/EmptyState.vue";
import HelpBubble from "../components/shared/HelpBubble.vue";
import Icon from "../components/shared/Icon.vue";
import LoadingPanel from "../components/shared/LoadingPanel.vue";
import LoadingSpinner from "../components/shared/LoadingSpinner.vue";
import MobileBackBar from "../components/shared/MobileBackBar.vue";
import PanelHeader from "../components/shared/PanelHeader.vue";
import SplitPane from "../components/shared/SplitPane.vue";
import StatusBadge from "../components/shared/StatusBadge.vue";
import { useGatesStore } from "../../ui/adapters/pinia/gates";
import { useOrgsStore } from "../../ui/adapters/pinia/orgs";
import { useAppStore } from "../../ui/adapters/pinia/app";
import { useOperationLoading } from "../composables/useOperationLoading";
import { pretty } from "../../core/utils/format";
import { isBadStatus, isGoodStatus } from "../../core/utils/status";

const gates = useGatesStore();
const orgs = useOrgsStore();
const app = useAppStore();
const { isLoading: loadingGates, loadingMessage: loadingGatesMessage } =
  useOperationLoading("Refreshing gates");
const reason = ref("");

async function resolve(action: "open" | "close") {
  if (!gates.selectedGate || !reason.value.trim()) {
    return;
  }

  const label = gates.selectedGate.label ?? gates.selectedGate.id ?? "selected gate";

  if (!window.confirm(`${action === "open" ? "Open" : "Close"} gate “${label}”?`)) {
    return;
  }

  await gates.resolveSelected(action, reason.value);
  reason.value = "";
}

async function removeSelected() {
  const gate = gates.selectedGate;

  if (!gate || !window.confirm("Delete the selected gate record? This does not resume its run.")) {
    return;
  }

  await gates.removeSelected();
}

async function refresh() {
  gates.clearGates();
  await gates.refreshGates();
}

onMounted(refresh);
watch(() => orgs.activeOrgId, refresh);
</script>
