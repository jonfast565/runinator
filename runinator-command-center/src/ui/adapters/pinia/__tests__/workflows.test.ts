import { afterEach, beforeEach, describe, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { useWorkflowsStore } from "../workflows";

vi.mock("../../../../core/api/commandCenterApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../../core/api/commandCenterApi")>()),
  closeGate: vi.fn(),
  compileWdl: vi.fn(),
  fetchGates: vi.fn(),
  fetchWorkflows: vi.fn(),
  fetchWorkflowRun: vi.fn(),
  openGate: vi.fn(),
  patchWorkflowRunDebug: vi.fn(),
  saveWorkflowWdl: vi.fn(),
  decompileToWdl: vi.fn(),
}));

import { decompileToWdl, fetchGates } from "../../../../core/api/commandCenterApi";
import { setWorkflowCatalogs } from "../../../../core/workflow/catalog-registry";
import { testNodeKindCatalog } from "../../../../core/workflow/__tests__/catalog-fixtures";
import { registerWorkflowAuthoringTests } from "./workflows-authoring.cases";
import { registerWorkflowRunStateTests } from "./workflows-run-state.cases";
import { registerWorkflowSyncTests } from "./workflows-sync.cases";

describe("workflow store adapter", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    setWorkflowCatalogs({ nodeKinds: testNodeKindCatalog, triggerKinds: [], enums: [] });
    useWorkflowsStore().clearServiceState({ discardDraft: true });
    vi.stubGlobal("window", {
      clearTimeout: () => undefined,
      setTimeout: (fn: () => void) => {
        fn();
        return 0;
      },
    });
    vi.clearAllMocks();
    vi.mocked(fetchGates).mockResolvedValue([]);
    vi.mocked(decompileToWdl).mockResolvedValue("workflow stub { start -> end }");
  });

  afterEach(() => {
    setWorkflowCatalogs({ nodeKinds: [], triggerKinds: [], enums: [] });
  });

  registerWorkflowRunStateTests();
  registerWorkflowAuthoringTests();
  registerWorkflowSyncTests();
});
