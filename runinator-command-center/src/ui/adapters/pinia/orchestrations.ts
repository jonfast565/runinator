import { defineStore } from "pinia";
import { ref } from "vue";
import type {
  OrchestrationBinding,
  OrchestrationCommand,
  OrchestrationEpoch,
  OrchestrationEvidence,
  OrchestrationReduction,
} from "../../../core/domain/models";
import {
  fetchOrchestration,
  fetchOrchestrationCommands,
  fetchOrchestrationEpochs,
  fetchOrchestrationEvents,
  fetchOrchestrationEvidence,
  fetchOrchestrations,
  sendOrchestrationIntent,
} from "../../../core/services/orchestrations";

export const useOrchestrationsStore = defineStore("orchestrations", () => {
  const bindings = ref<OrchestrationBinding[]>([]);
  const selectedId = ref<string | null>(null);
  const selected = ref<OrchestrationBinding | null>(null);
  const epochs = ref<OrchestrationEpoch[]>([]);
  const events = ref<OrchestrationReduction[]>([]);
  const evidence = ref<OrchestrationEvidence[]>([]);
  const commands = ref<OrchestrationCommand[]>([]);
  const loading = ref(false);
  const error = ref<string | null>(null);

  async function refresh(filters: Record<string, unknown> = {}): Promise<void> {
    loading.value = true;
    error.value = null;

    try {
      bindings.value = await fetchOrchestrations(filters);

      if (selectedId.value && bindings.value.some((item) => item.id === selectedId.value)) {
        await select(selectedId.value);
      } else if (bindings.value[0]) {
        await select(bindings.value[0].id);
      }
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : String(cause);
    } finally {
      loading.value = false;
    }
  }

  async function select(id: string): Promise<void> {
    selectedId.value = id;
    [selected.value, epochs.value, events.value, evidence.value, commands.value] = await Promise.all([
      fetchOrchestration(id),
      fetchOrchestrationEpochs(id),
      fetchOrchestrationEvents(id),
      fetchOrchestrationEvidence(id),
      fetchOrchestrationCommands(id),
    ]);
  }

  async function dispatch(intent: string, reason: string): Promise<void> {
    if (!selectedId.value) {
      return;
    }

    await sendOrchestrationIntent(selectedId.value, intent, reason);
    await select(selectedId.value);
  }

  return { bindings, selectedId, selected, epochs, events, evidence, commands, loading, error, refresh, select, dispatch };
});
