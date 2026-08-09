import { computed, ref, watch } from "vue";
import {
  fetchWorkflowNodeTransitions,
  fetchWorkflowRunTransitions,
} from "../../../core/api/commandCenterApi";
import type { NodeTransition, NodeTransitionStat } from "../../../core/domain/models";
import type { useWorkflowsStore } from "../../adapters/pinia/workflows";

type WorkflowsStore = ReturnType<typeof useWorkflowsStore>;

/** transition history and cross-run path statistics for an open workflow run. */
export function useWorkflowTransitionStats(workflows: WorkflowsStore) {
  const runTransitions = ref<NodeTransition[]>([]);
  const nodeStats = ref<NodeTransitionStat[]>([]);

  watch(
    () => workflows.workflowRunDetail?.run.id ?? null,
    async (runId) => {
      runTransitions.value = [];

      if (!runId) {
        return;
      }

      try {
        runTransitions.value = await fetchWorkflowRunTransitions(runId);
      } catch {
        runTransitions.value = [];
      }
    },
    { immediate: true },
  );

  watch(
    () => [workflows.workflowRunWorkflow?.id ?? null, workflows.selectedWorkflowRunNodeId] as const,
    async ([workflowId, nodeId]) => {
      nodeStats.value = [];

      if (!workflowId || !nodeId) {
        return;
      }

      try {
        nodeStats.value = await fetchWorkflowNodeTransitions(workflowId, nodeId);
      } catch {
        nodeStats.value = [];
      }
    },
    { immediate: true },
  );

  const sortedNodeStats = computed<NodeTransitionStat[]>(() =>
    [...nodeStats.value].sort((a, b) => b.count - a.count),
  );
  const nodeStatTotal = computed(() => nodeStats.value.reduce((sum, stat) => sum + stat.count, 0));

  function statPercent(count: number): string {
    return nodeStatTotal.value <= 0
      ? "0%"
      : `${String(Math.round((count / nodeStatTotal.value) * 100))}%`;
  }

  return { runTransitions, sortedNodeStats, statPercent };
}
