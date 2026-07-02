import { closeGate, deleteGate, fetchGates, openGate } from "../api/commandCenterApi";
import type { GateRecord } from "../domain/models";
import { createStore } from "./event-bus";
import type { AppService } from "./app";
import type { ConfirmContext } from "./operation-context";

export interface GatesState {
  gates: GateRecord[];
  selectedGate: GateRecord | null;
}

export function createGatesService(app: AppService) {
  const store = createStore<GatesState>({ gates: [], selectedGate: null });

  function filteredGates(query: string): GateRecord[] {
    if (!query) {
      return store.getState().gates;
    }

    const matches: GateRecord[] = [];

    for (const gate of store.getState().gates) {
      const haystack = [
        gate.id,
        gate.kind,
        gate.status,
        gate.label,
        gate.node_id,
        gate.workflow_run_id,
      ]
        .filter((value) => value !== undefined && value !== null)
        .map((value) => (typeof value === "string" ? value : String(value)).toLowerCase());

      if (haystack.some((value) => value.includes(query))) {
        matches.push(gate);
      }
    }

    return matches;
  }

  const service = {
    ...store,
    filteredGates,
    canResolveSelected() {
      const status = store.getState().selectedGate?.status ?? "";
      return Boolean(store.getState().selectedGate?.id) && ["pending", "closed"].includes(status);
    },
    setSelectedGate(gate: GateRecord | null) {
      store.setState((state) => ({ ...state, selectedGate: gate }));
    },
    async refreshGates() {
      const gates = await app.runOperation("Refreshing gates", fetchGates).catch(() => []);
      const selectedId = store.getState().selectedGate?.id;
      store.setState((state) => ({
        ...state,
        gates,
        selectedGate: gates.find((gate) => gate.id === selectedId) ?? gates.at(0) ?? null,
      }));
    },
    clearGates() {
      store.setState(() => ({ gates: [], selectedGate: null }));
    },
    async resolveSelected(action: "open" | "close", reason?: string) {
      const gateId = store.getState().selectedGate?.id ?? "";

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
      await service.refreshGates();
    },
    async removeSelected(confirm: ConfirmContext) {
      const gateId = store.getState().selectedGate?.id ?? "";

      if (!gateId) {
        app.setError("No gate selected");
        return;
      }

      if (!confirm.confirm("Delete this gate record?")) {
        return;
      }

      await app
        .runOperation("Deleting gate", () => deleteGate(gateId))
        .catch((error: unknown) => {
          app.setError(String(error));
        });
      store.setState((state) => ({
        ...state,
        gates: state.gates.filter((gate) => gate.id !== gateId),
        selectedGate: state.gates.at(0) ?? null,
      }));
      await service.refreshGates();
    },
  };

  return service;
}

export type GatesService = ReturnType<typeof createGatesService>;
