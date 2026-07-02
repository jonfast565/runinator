import { defineStore } from "pinia";
import { computed, ref } from "vue";
import { closeGate, deleteGate, fetchGates, openGate } from "../api/commandCenterApi";
import type { GateRecord } from "../types/models";
import { useAppStore } from "./app";

// dedicated store for the gates view. unlike the generic resources store, gates carry their own
// open/close actions (a gate blocks a workflow node until it is opened).
export const useGatesStore = defineStore("gates", () => {
  const gates = ref<GateRecord[]>([]);
  const selectedGate = ref<GateRecord | null>(null);
  const app = useAppStore();

  const filteredGates = computed(() => {
    const query = app.normalizedSearch;

    if (!query) {
      return gates.value;
    }

    return gates.value.filter((gate) =>
      [gate.id, gate.kind, gate.status, gate.label, gate.node_id, gate.workflow_run_id]
        .filter((value) => value !== undefined && value !== null)
        .some((value) => value.toLowerCase().includes(query)),
    );
  });

  // a gate is resolvable from the ui only while it is still pending/closed.
  const canResolveSelected = computed(() => {
    const status = selectedGate.value?.status ?? "";
    return Boolean(selectedGate.value?.id) && ["pending", "closed"].includes(status);
  });

  async function refreshGates() {
    gates.value = await app.runOperation("Refreshing gates", fetchGates).catch(() => []);
    // keep the selection pinned to the same gate id across refreshes when possible.
    const selectedId = selectedGate.value?.id;
    selectedGate.value =
      gates.value.find((gate) => gate.id === selectedId) ?? gates.value.at(0) ?? null;
  }

  function clearGates() {
    gates.value = [];
    selectedGate.value = null;
  }

  async function resolveSelected(action: "open" | "close", reason?: string) {
    const gateId = selectedGate.value?.id ?? "";

    if (!gateId) {
      app.setError("No gate selected");
      return;
    }

    const trimmed = reason?.trim() ? reason.trim() : undefined;
    const response = await app.runOperation(
      action === "open" ? "Opening gate" : "Closing gate",
      () => (action === "open" ? openGate(gateId, trimmed) : closeGate(gateId, trimmed)),
    );
    app.setStatus(response.message);
    await refreshGates();
  }

  async function removeSelected() {
    const gateId = selectedGate.value?.id ?? "";

    if (!gateId) {
      app.setError("No gate selected");
      return;
    }

    if (!window.confirm("Delete this gate record?")) {
      return;
    }

    await app
      .runOperation("Deleting gate", () => deleteGate(gateId))
      .catch((error: unknown) => {
        app.setError(String(error));
      });
    gates.value = gates.value.filter((gate) => gate.id !== gateId);
    selectedGate.value = gates.value.at(0) ?? null;
    await refreshGates();
  }

  return {
    gates,
    selectedGate,
    filteredGates,
    canResolveSelected,
    refreshGates,
    clearGates,
    resolveSelected,
    removeSelected,
  };
});
