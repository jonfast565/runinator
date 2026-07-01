<template>
  <section class="pane resources-pane">
    <SplitPane class="split" storage-key="command-center.gates.split" :initial-first-pct="58" :min-first="420" :min-second="340" collapsible-second>
      <template #first>
        <div class="panel">
          <div class="panel-toolbar">
            <h2>Gates</h2>
            <div class="btn-row">
              <button class="btn" @click="gates.refreshGates">
                <Icon name="refresh" />
                <span>Refresh</span>
              </button>
              <button class="btn btn-primary" :disabled="!gates.canResolveSelected" @click="resolve('open')">
                <Icon name="approve" />
                <span>Open</span>
              </button>
              <button class="btn btn-danger" :disabled="!gates.canResolveSelected" @click="resolve('close')">
                <Icon name="reject" />
                <span>Close</span>
              </button>
              <button class="btn" :disabled="!gates.selectedGate" @click="gates.removeSelected()">
                <Icon name="trash" />
                <span>Delete</span>
              </button>
            </div>
          </div>
          <p class="gates-hint">
            A gate blocks its workflow node until it opens. <strong>condition</strong> gates open automatically; open or
            close <strong>manual</strong> and <strong>external</strong> gates here.
          </p>
          <DataTable>
            <table>
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
                <tr
                  v-for="gate in gates.filteredGates"
                  :key="String(gate.id ?? JSON.stringify(gate))"
                  :class="{ selected: gates.selectedGate === gate, danger: isBadStatus(gate.status), success: isGoodStatus(gate.status) }"
                  @click="gates.selectedGate = gate"
                >
                  <td><StatusBadge :status="gate.status" /></td>
                  <td>{{ gate.kind ?? "" }}</td>
                  <td>{{ gate.label ?? "" }}</td>
                  <td>{{ gate.node_id ?? "" }}</td>
                  <td class="gate-run-cell">{{ gate.workflow_run_id ?? "" }}</td>
                </tr>
              </tbody>
            </table>
          </DataTable>
        </div>
      </template>
      <template #second>
        <div class="panel details">
          <h2>Gate Detail</h2>
          <label class="gate-reason">
            Reason (optional)
            <input v-model="reason" placeholder="Why are you opening/closing this gate?" />
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
import Icon from "../components/shared/Icon.vue";
import SplitPane from "../components/shared/SplitPane.vue";
import StatusBadge from "../components/shared/StatusBadge.vue";
import { useGatesStore } from "../stores/gates";
import { useOrgsStore } from "../stores/orgs";
import { pretty } from "../utils/format";
import { isBadStatus, isGoodStatus } from "../utils/status";

const gates = useGatesStore();
const orgs = useOrgsStore();
const reason = ref("");

async function resolve(action: "open" | "close") {
  await gates.resolveSelected(action, reason.value);
  reason.value = "";
}

async function refresh() {
  gates.clearGates();
  await gates.refreshGates();
}

onMounted(refresh);
watch(() => orgs.activeOrgId, refresh);
</script>

<style scoped>
.gates-hint {
  margin: 0;
  color: var(--text-muted);
  font-size: 12px;
}

.gate-run-cell {
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 11px;
}

.gate-reason {
  display: grid;
  gap: 4px;
  color: var(--text-muted);
  font-size: 12px;
}
</style>
