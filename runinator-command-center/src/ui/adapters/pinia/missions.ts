import { defineStore } from "pinia";
import { ref, shallowRef } from "vue";
import type {
  IngressResponse,
  OrchestrationBinding,
  OrchestrationEpoch,
  OrchestrationEvidence,
  Pipeline,
} from "../../../core/domain/models";
import {
  fetchMissionDetail,
  fetchMissionPipelines,
  fetchMissions,
  sendMissionIntent,
  sendMissionSteering,
  startMission,
} from "../../../core/services";
import type { MissionEffectActivity, StartMissionInput } from "../../../core/services";

export const useMissionsStore = defineStore("missions", () => {
  const missionPipelines = ref<Pipeline[]>([]);
  const missions = ref<OrchestrationBinding[]>([]);
  const selectedId = ref<string | null>(null);
  const selected = shallowRef<OrchestrationBinding | null>(null);
  const epochs = ref<OrchestrationEpoch[]>([]);
  const evidence = ref<OrchestrationEvidence[]>([]);
  const effects = ref<MissionEffectActivity[]>([]);
  const loading = ref(false);
  const detailLoading = ref(false);
  const error = ref<string | null>(null);
  let selectionVersion = 0;

  function clearSelection(): void {
    selectionVersion += 1;
    selectedId.value = null;
    selected.value = null;
    epochs.value = [];
    evidence.value = [];
    effects.value = [];
    detailLoading.value = false;
  }

  async function refresh(): Promise<void> {
    loading.value = true;
    error.value = null;

    try {
      const [pipelines, bindings] = await Promise.all([fetchMissionPipelines(), fetchMissions()]);
      missionPipelines.value = pipelines;
      missions.value = bindings;

      if (selectedId.value && bindings.some((binding) => binding.id === selectedId.value)) {
        await select(selectedId.value);
      } else if (bindings[0]) {
        await select(bindings[0].id);
      } else {
        clearSelection();
      }
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : String(cause);
    } finally {
      loading.value = false;
    }
  }

  async function select(id: string): Promise<void> {
    const requestVersion = ++selectionVersion;
    selectedId.value = id;
    detailLoading.value = true;
    error.value = null;

    try {
      const detail = await fetchMissionDetail(id);

      if (requestVersion !== selectionVersion) {
        return;
      }

      selected.value = detail.binding;
      epochs.value = detail.epochs.sort((left, right) => right.epoch - left.epoch);
      evidence.value = detail.evidence.sort((left, right) =>
        right.created_at.localeCompare(left.created_at),
      );
      effects.value = detail.effects;
    } catch (cause) {
      if (requestVersion === selectionVersion) {
        error.value = cause instanceof Error ? cause.message : String(cause);
      }
    } finally {
      if (requestVersion === selectionVersion) {
        detailLoading.value = false;
      }
    }
  }

  async function start(input: StartMissionInput): Promise<IngressResponse> {
    return startMission(input);
  }

  async function steer(missionId: string, message: string): Promise<void> {
    await sendMissionSteering(missionId, message);
  }

  async function intent(missionId: string, name: string, reason: string): Promise<void> {
    await sendMissionIntent(missionId, name, reason);
  }

  return {
    missionPipelines,
    missions,
    selectedId,
    selected,
    epochs,
    evidence,
    effects,
    loading,
    detailLoading,
    error,
    refresh,
    select,
    start,
    steer,
    intent,
  };
});
