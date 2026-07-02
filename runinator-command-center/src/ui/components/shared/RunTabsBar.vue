<template>
  <div v-if="workflows.openRunIds.length > 0" class="run-tabs">
    <div
      v-for="runId in workflows.openRunIds"
      :key="runId"
      :class="['run-tab', { active: runId === workflows.selectedWorkflowRunId }]"
      :title="tabTitle(runId)"
      @click="workflows.activateRunTab(runId)"
    >
      <span class="run-tab-dot" :class="statusClass(runId)"></span>
      <span class="run-tab-label">{{ labelFor(runId) }}</span>
      <button
        class="btn-close"
        :title="`Close run ${runId}`"
        @click.stop="workflows.closeRunTab(runId)"
      >
        <Icon name="x" :size="11" />
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
import Icon from "./Icon.vue";
import { useWorkflowsStore } from "../../../ui/adapters/pinia/workflows";

const workflows = useWorkflowsStore();

function labelFor(runId: string): string {
  const detail = workflows.runDetailById.get(runId);
  const summary = workflows.workflowRuns.find((run) => run.id === runId);
  const name = (detail?.run.name ?? summary?.name)?.trim();
  return name ?? `Run #${runId}`;
}

function tabTitle(runId: string): string {
  const status = statusFor(runId) ?? "unknown";
  return `Run ${runId} · ${status}`;
}

function statusFor(runId: string): string | undefined {
  const detail = workflows.runDetailById.get(runId);

  if (detail?.run.status) {
    return detail.run.status;
  }

  const summary = workflows.workflowRuns.find((run) => run.id === runId);
  return summary?.status;
}

function statusClass(runId: string): string {
  const status = statusFor(runId);

  if (!status) {
    return "pending";
  }

  if (status === "succeeded") {
    return "ok";
  }

  if (status === "failed" || status === "timed_out") {
    return "fail";
  }

  if (status === "canceled") {
    return "warn";
  }

  if (status === "running" || status === "queued" || status === "debug_paused") {
    return "live";
  }

  return "pending";
}
</script>

<style scoped>
.run-tabs {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 4px 6px 0;
  overflow-x: auto;
  border-bottom: 1px solid var(--border);
  background: var(--surface-subtle);
}

.run-tab {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 6px 8px;
  border: 1px solid transparent;
  border-bottom: none;
  border-top-left-radius: 6px;
  border-top-right-radius: 6px;
  background: transparent;
  color: var(--text-subtle);
  cursor: pointer;
  font-size: 12px;
  white-space: nowrap;
  max-width: 220px;
}

.run-tab:hover {
  background: var(--surface-muted);
}

.run-tab.active {
  background: var(--surface);
  border-color: var(--border);
  color: var(--text);
  position: relative;
  bottom: -1px;
}

.run-tab-label {
  overflow: hidden;
  text-overflow: ellipsis;
}

.run-tab-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--border-strong);
  flex: 0 0 auto;
}

.run-tab-dot.ok {
  background: var(--success-fg);
}

.run-tab-dot.fail {
  background: var(--danger-solid);
}

.run-tab-dot.warn {
  background: var(--warn-solid);
}

.run-tab-dot.live {
  background: var(--accent);
  box-shadow: 0 0 0 2px rgba(37, 99, 235, 0.18);
}

.run-tab .btn-close {
  width: 16px;
  height: 16px;
  background: transparent;
  border: 0;
  color: inherit;
  border-radius: 3px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
}

.run-tab .btn-close:hover {
  background: rgba(15, 23, 42, 0.1);
}
</style>
