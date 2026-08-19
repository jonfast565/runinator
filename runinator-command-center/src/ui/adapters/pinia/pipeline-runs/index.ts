import { defineStore } from "pinia";
import { ref } from "vue";
import type { Pipeline, PipelineRun, PipelineRunDetail } from "../../../../core/domain/models";
import {
  cancelPipelineRun as cancelPipelineRunService,
  createPipelineRun as createPipelineRunService,
  fetchPipelineRun,
  fetchPipelineRuns,
  fetchPipelines,
  resolvePipelineRun as resolvePipelineRunService,
  retryPipelineMember as retryPipelineMemberService,
} from "../../../../core/services/pipeline";

// the pipeline-runs monitor store. mirrors the workflow-runs list+detail model: a flat list of recent
// pipeline runs plus the selected run's detail (its member workflow runs), refreshed on demand and by
// the realtime event router. the operator can start a manual run or cancel an in-flight one.
export const usePipelineRunsStore = defineStore("pipelineRuns", () => {
  const runs = ref<PipelineRun[]>([]);
  const pipelines = ref<Pipeline[]>([]);
  const loading = ref(false);
  const error = ref<string | null>(null);
  const selectedRunId = ref<string | null>(null);
  const detail = ref<PipelineRunDetail | null>(null);
  const detailLoading = ref(false);

  async function refresh(): Promise<void> {
    loading.value = true;
    error.value = null;

    try {
      const [runList, pipelineList] = await Promise.all([fetchPipelineRuns(), fetchPipelines()]);

      runs.value = runList;
      pipelines.value = pipelineList;

      // keep the open detail in sync, and default the selection to the newest run.
      if (selectedRunId.value && runList.some((run) => run.id === selectedRunId.value)) {
        await loadDetail(selectedRunId.value);
      } else if (!selectedRunId.value && runList.length > 0) {
        await selectRun(runList[0].id);
      }
    } catch (err) {
      error.value = err instanceof Error ? err.message : String(err);
    } finally {
      loading.value = false;
    }
  }

  async function selectRun(pipelineRunId: string): Promise<void> {
    selectedRunId.value = pipelineRunId;
    await loadDetail(pipelineRunId);
  }

  async function loadDetail(pipelineRunId: string): Promise<void> {
    detailLoading.value = true;

    try {
      detail.value = await fetchPipelineRun(pipelineRunId);
    } catch (err) {
      error.value = err instanceof Error ? err.message : String(err);
    } finally {
      detailLoading.value = false;
    }
  }

  async function startRun(pipelineId: string): Promise<void> {
    const run = await createPipelineRunService(pipelineId);
    await refresh();
    await selectRun(run.id);
  }

  async function cancelRun(pipelineRunId: string): Promise<void> {
    await cancelPipelineRunService(pipelineRunId);
    await refresh();
  }

  async function retryMember(
    pipelineRunId: string,
    memberKey: string,
    parameters: Record<string, unknown> = {},
  ): Promise<void> {
    await retryPipelineMemberService(pipelineRunId, memberKey, parameters);
    await refresh();
  }

  // resolve a pending inquiry (a member with the `inquire` failure mode paused the run).
  async function resolveRun(
    pipelineRunId: string,
    decision: "continue" | "abort",
  ): Promise<void> {
    await resolvePipelineRunService(pipelineRunId, decision);
    await refresh();
  }

  // refetch the open detail when one of its member workflow runs changes, so step status/timing in
  // the detail track live rather than waiting on the next pipeline-run event or fallback poll.
  async function refreshDetailIfMember(workflowRunId: string): Promise<void> {
    if (selectedRunId.value && detail.value?.members.some((member) => member.id === workflowRunId)) {
      await loadDetail(selectedRunId.value);
    }
  }

  return {
    runs,
    pipelines,
    loading,
    error,
    selectedRunId,
    detail,
    detailLoading,
    refresh,
    selectRun,
    startRun,
    cancelRun,
    resolveRun,
    retryMember,
    refreshDetailIfMember,
  };
});
