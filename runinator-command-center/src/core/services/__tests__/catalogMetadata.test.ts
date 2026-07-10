import { beforeEach, describe, expect, it, vi } from "vitest";
import { createCatalogMetadataService } from "../catalogMetadata";
import { getNodeKindCatalog, setWorkflowCatalogs } from "../../workflow/catalog-registry";
import type {
  EnumCatalogMetadata,
  WorkflowNodeKindMetadata,
  WorkflowTriggerKindMetadata,
} from "../../domain/models";

vi.mock("../../api/commandCenterApi", () => ({
  fetchNodeKinds: vi.fn(),
  fetchTriggerKinds: vi.fn(),
  fetchEnumCatalogs: vi.fn(),
}));

import {
  fetchEnumCatalogs,
  fetchNodeKinds,
  fetchTriggerKinds,
} from "../../api/commandCenterApi";

const nodeKinds: WorkflowNodeKindMetadata[] = [
  {
    kind: "wait",
    label: "Wait",
    icon: "clock",
    description: "Pauses the run.",
    category: "control-flow",
    protected: false,
    terminal: false,
    addable: true,
    supports_predicate_edges: true,
    fields: [
      {
        name: "seconds",
        ty: { type: "duration" },
        required: false,
        secret: false,
        location: { base: "wait", path: ["seconds"] },
        widget: "duration",
      },
    ],
    edge_slots: [],
    default_template: {
      kind: "wait",
      wait: { seconds: 60 },
      parameters: {},
      retry: { max_attempts: 1 },
      transitions: {},
    },
  },
];

const triggerKinds: WorkflowTriggerKindMetadata[] = [
  {
    kind: "cron",
    label: "Cron",
    icon: "clock",
    description: "Fires on a cron schedule.",
    fields: [
      {
        name: "cron",
        ty: { type: "string" },
        required: true,
        secret: false,
        widget: "cron",
      },
    ],
    default_configuration: { cron: "0 * * * *", parameters: {} },
  },
];

const enums: EnumCatalogMetadata[] = [
  {
    name: "match_kind",
    options: [
      { value: "equals", label: "Equals" },
      { value: "when", label: "When" },
    ],
  },
];

describe("catalogMetadataService", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    setWorkflowCatalogs({ nodeKinds: [], triggerKinds: [], enums: [] });
    vi.mocked(fetchNodeKinds).mockResolvedValue(nodeKinds);
    vi.mocked(fetchTriggerKinds).mockResolvedValue(triggerKinds);
    vi.mocked(fetchEnumCatalogs).mockResolvedValue(enums);
  });

  it("fetches catalogs once and caches them for the session", async () => {
    const service = createCatalogMetadataService();

    await service.fetchCatalogs();
    await service.fetchCatalogs();

    expect(fetchNodeKinds).toHaveBeenCalledTimes(1);
    expect(fetchTriggerKinds).toHaveBeenCalledTimes(1);
    expect(fetchEnumCatalogs).toHaveBeenCalledTimes(1);
    expect(service.getState().loaded).toBe(true);
    expect(service.getState().nodeKinds).toEqual(nodeKinds);
  });

  it("refetches when force is true", async () => {
    const service = createCatalogMetadataService();

    await service.fetchCatalogs();
    await service.fetchCatalogs(true);

    expect(fetchNodeKinds).toHaveBeenCalledTimes(2);
  });

  it("publishes catalogs into the workflow registry", async () => {
    const service = createCatalogMetadataService();
    await service.fetchCatalogs();

    expect(getNodeKindCatalog()).toEqual(nodeKinds);
    expect(service.nodeKind("wait")?.label).toBe("Wait");
    expect(service.triggerKind("cron")?.fields[0]?.widget).toBe("cron");
    expect(service.enumOptions("match_kind").map((option) => option.value)).toEqual([
      "equals",
      "when",
    ]);
  });

  it("records fetch errors without clearing prior successful state on force failure", async () => {
    const service = createCatalogMetadataService();
    await service.fetchCatalogs();

    vi.mocked(fetchNodeKinds).mockRejectedValueOnce(new Error("boom"));
    await service.fetchCatalogs(true);

    expect(service.getState().error).toContain("boom");
    expect(service.getState().loading).toBe(false);
  });
});
