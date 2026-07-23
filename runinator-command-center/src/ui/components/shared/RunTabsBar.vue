<template>
  <div
    v-if="workflows.openRunIds.length > 0"
    class="flex items-center gap-1 overflow-x-auto border-b border-border bg-surface-subtle px-1.5 pt-1"
  >
    <div
      v-for="runId in workflows.openRunIds"
      :key="runId"
      :class="[
        'inline-flex max-w-[220px] cursor-pointer items-center gap-1.5 whitespace-nowrap rounded-t-md border border-transparent px-2 py-1.5 text-xs text-fg-subtle',
        runId === workflows.selectedWorkflowRunId
          ? 'relative -bottom-px border-border border-b-0 bg-surface text-fg'
          : 'hover:bg-surface-muted',
      ]"
      :title="tabTitle(runId)"
      @click="workflows.activateRunTab(runId)"
    >
      <span class="size-2 shrink-0 rounded-full" :class="statusClass(runId)"></span>
      <span class="overflow-hidden text-ellipsis">{{ labelFor(runId) }}</span>
      <button
        class="inline-flex size-4 shrink-0 cursor-pointer items-center justify-center rounded-sm border-0 bg-transparent p-0 text-inherit hover:bg-black/10 dark:hover:bg-white/10"
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
    return "bg-border-strong";
  }

  if (status === "succeeded") {
    return "bg-success-fg";
  }

  if (status === "failed" || status === "timed_out") {
    return "bg-danger";
  }

  if (status === "canceled") {
    return "bg-warn";
  }

  if (status === "running" || status === "queued" || status === "debug_paused") {
    return "bg-accent shadow-[0_0_0_2px_rgba(37,99,235,0.18)]";
  }

  return "bg-border-strong";
}
</script>
