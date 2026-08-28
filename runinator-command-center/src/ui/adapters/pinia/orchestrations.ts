import { defineStore } from "pinia";
import { ref, shallowRef } from "vue";
import type {
  AdapterDefinition,
  AdapterKindMetadata,
  AdapterRevision,
  ExternalOperation,
  OrchestrationBinding,
  OrchestrationCommand,
  OrchestrationCorrelationAlias,
  OrchestrationEpoch,
  OrchestrationEvidence,
  OrchestrationReduction,
  WorkspaceLease,
} from "../../../core/domain/models";
import {
  addOrchestrationAlias,
  applyAdapter,
  deleteAdapter,
  deleteOrchestrationAlias,
  fetchAdapterKinds,
  fetchAdapterRevisions,
  fetchAdapters,
  fetchExternalOperations,
  fetchOrchestration,
  fetchOrchestrationAliases,
  fetchOrchestrationCommands,
  fetchOrchestrationEpochs,
  fetchOrchestrationEvents,
  fetchOrchestrationEvidence,
  fetchOrchestrationWorkspaces,
  fetchOrchestrations,
  resolveExternalOperation,
  requeueOrchestration,
  sendOrchestrationIntent,
  setAdapterEnabled,
  testAdapter,
} from "../../../core/services/orchestrations";
import type { AdapterApplyInput } from "../../../core/api/commandCenterApi";

export const useOrchestrationsStore = defineStore("orchestrations", () => {
  const bindings = ref<OrchestrationBinding[]>([]);
  const selectedId = ref<string | null>(null);
  const selected = shallowRef<OrchestrationBinding | null>(null);
  const epochs = ref<OrchestrationEpoch[]>([]);
  const events = ref<OrchestrationReduction[]>([]);
  const evidence = ref<OrchestrationEvidence[]>([]);
  const commands = ref<OrchestrationCommand[]>([]);
  const operations = ref<ExternalOperation[]>([]);
  const workspaces = ref<WorkspaceLease[]>([]);
  const aliases = ref<OrchestrationCorrelationAlias[]>([]);
  const adapterKinds = shallowRef<AdapterKindMetadata[]>([]);
  const adapters = ref<AdapterDefinition[]>([]);
  const selectedAdapterId = ref<string | null>(null);
  const selectedAdapter = ref<AdapterDefinition | null>(null);
  const adapterRevisions = ref<AdapterRevision[]>([]);
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
    [selected.value, epochs.value, events.value, evidence.value, commands.value, operations.value, workspaces.value, aliases.value] = await Promise.all([
      fetchOrchestration(id),
      fetchOrchestrationEpochs(id),
      fetchOrchestrationEvents(id),
      fetchOrchestrationEvidence(id),
      fetchOrchestrationCommands(id),
      fetchExternalOperations(id),
      fetchOrchestrationWorkspaces(id),
      fetchOrchestrationAliases(id),
    ]);
  }

  function currentBinding(): OrchestrationBinding | null {
    return selected.value;
  }

  async function dispatch(intent: string, reason: string, payload: unknown = {}): Promise<void> {
    if (!selectedId.value) {
      return;
    }

    await sendOrchestrationIntent(selectedId.value, intent, reason, payload);
    await select(selectedId.value);
  }

  async function requeue(reason: string): Promise<void> {
    if (!selectedId.value) {
      return;
    }

    const next = await requeueOrchestration(selectedId.value, reason);
    selectedId.value = next.id;
    await refresh();
  }

  async function addAlias(source: string, scope: string, correlationKey: string): Promise<void> {
    if (!selectedId.value) {
      return;
    }

    await addOrchestrationAlias(selectedId.value, source, scope, correlationKey);
    aliases.value = await fetchOrchestrationAliases(selectedId.value);
  }

  async function removeAlias(aliasId: string): Promise<void> {
    if (!selectedId.value) {
      return;
    }

    await deleteOrchestrationAlias(selectedId.value, aliasId);
    aliases.value = await fetchOrchestrationAliases(selectedId.value);
  }

  async function refreshAdapters(): Promise<void> {
    loading.value = true;
    error.value = null;

    try {
      const [catalog, definitions] = await Promise.all([
        fetchAdapterKinds(),
        fetchAdapters(),
      ]);
      adapters.value = definitions;
      adapterKinds.value = catalog
        .filter((entry) => entry.healthy && !entry.error)
        .map((entry) => entry.metadata);

      if (selectedAdapterId.value && adapters.value.some((item) => item.id === selectedAdapterId.value)) {
        await selectAdapter(selectedAdapterId.value);
      } else if (adapters.value[0]) {
        await selectAdapter(adapters.value[0].id);
      } else {
        selectedAdapterId.value = null;
        selectedAdapter.value = null;
        adapterRevisions.value = [];
      }
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : String(cause);
    } finally {
      loading.value = false;
    }
  }

  async function selectAdapter(id: string): Promise<void> {
    selectedAdapterId.value = id;
    selectedAdapter.value = adapters.value.find((item) => item.id === id) ?? null;
    adapterRevisions.value = await fetchAdapterRevisions(id);
  }

  async function saveAdapter(input: AdapterApplyInput, adapterId?: string): Promise<void> {
    const saved = await applyAdapter(input, adapterId);
    selectedAdapterId.value = saved.id;
    await refreshAdapters();
  }

  async function toggleAdapter(adapter: AdapterDefinition): Promise<void> {
    await setAdapterEnabled(adapter.id, !adapter.enabled);
    await refreshAdapters();
  }

  async function removeAdapter(adapter: AdapterDefinition): Promise<void> {
    await deleteAdapter(adapter.id);
    selectedAdapterId.value = null;
    await refreshAdapters();
  }

  async function runAdapterTest(adapterId: string, headers: Record<string, string>, bodyBase64: string): Promise<unknown> {
    return testAdapter(adapterId, headers, bodyBase64);
  }

  async function resolveOperation(
    operation: ExternalOperation,
    resolution: "succeeded" | "failed" | "retry",
    reason: string,
    receipt: unknown,
  ): Promise<void> {
    await resolveExternalOperation(operation.binding_id, operation.id, resolution, reason, receipt);

    if (selectedId.value) {
      await select(selectedId.value);
    }
  }

  return {
    bindings, selectedId, selected, epochs, events, evidence, commands, operations, workspaces, aliases,
    adapterKinds, adapters, selectedAdapterId, selectedAdapter, adapterRevisions,
    loading, error, refresh, select, currentBinding, dispatch, requeue, addAlias, removeAlias, refreshAdapters, selectAdapter, saveAdapter,
    toggleAdapter, removeAdapter, runAdapterTest, resolveOperation,
  };
});
