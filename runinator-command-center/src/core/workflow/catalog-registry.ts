import type {
  WorkflowNodeKindMetadata,
  WorkflowTriggerKindMetadata,
  EnumCatalogMetadata,
} from "../domain/models";

// session cache of the backend ui catalogs. populated when catalogMetadataService.fetchCatalogs
// succeeds so pure workflow helpers (create, edge options, summaries) can read metadata without
// threading the catalog through every call site.

let nodeKinds: WorkflowNodeKindMetadata[] = [];
let triggerKinds: WorkflowTriggerKindMetadata[] = [];
let enums: EnumCatalogMetadata[] = [];

export function setWorkflowCatalogs(input: {
  nodeKinds: WorkflowNodeKindMetadata[];
  triggerKinds: WorkflowTriggerKindMetadata[];
  enums: EnumCatalogMetadata[];
}) {
  nodeKinds = input.nodeKinds;
  triggerKinds = input.triggerKinds;
  enums = input.enums;
}

export function getNodeKindCatalog(): WorkflowNodeKindMetadata[] {
  return nodeKinds;
}

export function getTriggerKindCatalog(): WorkflowTriggerKindMetadata[] {
  return triggerKinds;
}

export function getEnumCatalogs(): EnumCatalogMetadata[] {
  return enums;
}

export function findNodeKindMetadata(kind: string): WorkflowNodeKindMetadata | undefined {
  return nodeKinds.find((entry) => entry.kind === kind);
}

export function findTriggerKindMetadata(kind: string): WorkflowTriggerKindMetadata | undefined {
  return triggerKinds.find((entry) => entry.kind === kind);
}

export function enumOptions(name: string) {
  return enums.find((entry) => entry.name === name)?.options ?? [];
}

export function addableNodeKinds(): string[] {
  return nodeKinds.filter((entry) => entry.addable).map((entry) => entry.kind);
}

export function isNodeCatalogLoaded(): boolean {
  return nodeKinds.length > 0;
}

export function isTriggerCatalogLoaded(): boolean {
  return triggerKinds.length > 0;
}

export function isEnumCatalogLoaded(): boolean {
  return enums.length > 0;
}
