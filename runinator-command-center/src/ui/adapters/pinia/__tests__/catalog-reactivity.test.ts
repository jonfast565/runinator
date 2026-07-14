import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { computed } from "vue";
import { useCatalogMetadataStore } from "../catalogMetadata";
import type {
  EnumCatalogMetadata,
  WorkflowNodeKindMetadata,
} from "../../../../core/domain/models";

vi.mock("../../../../core/api/commandCenterApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../../core/api/commandCenterApi")>()),
  fetchNodeKinds: vi.fn(),
  fetchTriggerKinds: vi.fn(),
  fetchEnumCatalogs: vi.fn(),
}));

import {
  fetchNodeKinds,
  fetchTriggerKinds,
  fetchEnumCatalogs,
} from "../../../../core/api/commandCenterApi";

function nodeMeta(kind: string): WorkflowNodeKindMetadata {
  return {
    kind: kind as WorkflowNodeKindMetadata["kind"],
    label: kind,
    icon: "box",
    description: "",
    category: "control-flow",
    protected: false,
    terminal: false,
    addable: true,
    supports_predicate_edges: true,
    fields: [],
    edge_slots: [],
    default_template: { kind },
  };
}

const matchEnum: EnumCatalogMetadata = {
  name: "match_kind",
  options: [{ value: "equals", label: "Equals" }],
};

describe("catalog metadata store reactivity", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.mocked(fetchTriggerKinds).mockResolvedValue([]);
    vi.mocked(fetchEnumCatalogs).mockResolvedValue([]);
  });

  // regression: the store's lookup methods used to read the service's non-reactive getState(), so a
  // component computed() calling nodeKind()/enumOptions() cached its first (empty) result and never
  // refreshed once the catalog finished loading — leaving palettes, node icons, and selects blank.
  it("computed over nodeKind() re-evaluates after the catalog loads", async () => {
    const store = useCatalogMetadataStore();
    const waitMeta = computed(() => store.nodeKind("wait"));

    expect(waitMeta.value).toBeUndefined();

    vi.mocked(fetchNodeKinds).mockResolvedValue([nodeMeta("wait")]);
    await store.fetchCatalogs(true);

    expect(waitMeta.value?.kind).toBe("wait");
  });

  it("computed over enumOptions() re-evaluates after the catalog loads", async () => {
    const store = useCatalogMetadataStore();
    const options = computed(() => store.enumOptions("match_kind"));

    expect(options.value).toEqual([]);

    vi.mocked(fetchNodeKinds).mockResolvedValue([]);
    vi.mocked(fetchEnumCatalogs).mockResolvedValue([matchEnum]);
    await store.fetchCatalogs(true);

    expect(options.value.map((option) => option.value)).toEqual(["equals"]);
  });
});
