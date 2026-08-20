import type {
  JsonRecord,
  JsonValue,
  ProviderMetadata,
  RuninatorType,
  WorkflowRunDetail,
} from "../domain/models";
import { nodeRefId, workflowNodeActionConfig } from "../workflow/index";
import { findNodeKindMetadata } from "../workflow/catalog-registry";
import { nodeTargets } from "../workflow/interrupt-regions";

// the data an expression editor needs to enumerate the references in scope at a given node, plus an
// optional sample context (a prior run's data) the editor can resolve expressions against.
export interface WorkflowExpressionEditorContext {
  workflowInputType?: RuninatorType | null;
  nodes?: JsonRecord[];
  currentNodeId?: string | null;
  providers?: ProviderMetadata[];
  sampleContext?: JsonRecord | null;
}

// a single insertable reference: what to show, the REXRAP text to splice in, and its declared type.
export interface WorkflowReference {
  label: string;
  insert: string;
  type: string;
}

// references sharing an origin (workflow parameters, a prior node's output, or the run roots).
export interface ReferenceGroup {
  title: string;
  references: WorkflowReference[];
}

// the always-available reference roots, independent of schema or prior nodes.
const STATIC_ROOTS: WorkflowReference[] = [
  { label: "prev", insert: "prev", type: "previous node output" },
  { label: "run", insert: "run", type: "workflow run state" },
  { label: "config", insert: "config", type: "configuration value" },
  { label: "secret", insert: "secret", type: "secret reference" },
  { label: "interrupt", insert: "interrupt", type: "interrupt context" },
];

// references for every field of the workflow parameter struct, flattened by dotted path.
export function paramsReferences(ty: RuninatorType | null | undefined): WorkflowReference[] {
  if (ty?.type !== "struct") {
    return [];
  }

  const references: WorkflowReference[] = [];
  collectParamFields(ty, ["params"], references);
  return references;
}

function collectParamFields(ty: RuninatorType, path: string[], references: WorkflowReference[]) {
  if (ty.type !== "struct") {
    return;
  }

  for (const [name, field] of Object.entries(ty.fields)) {
    const nextPath = [...path, name];
    const dotted = nextPath.join(".");
    references.push({ label: dotted, insert: dotted, type: describeType(field.ty) });
    collectParamFields(field.ty, nextPath, references);
  }
}

// references for the declared outputs of every prior action node (the current node is excluded).
export function nodeOutputReferences(
  context?: WorkflowExpressionEditorContext,
): (WorkflowReference & { node: string })[] {
  const nodes = context?.nodes ?? [];
  const providers = context?.providers ?? [];
  const references: (WorkflowReference & { node: string })[] = [];
  const available = upstreamNodeIds(nodes, context?.currentNodeId ?? null);
  const scopedLoops = new Set(
    loopRegions(nodes)
      .filter((region) => context?.currentNodeId && region.nodes.has(context.currentNodeId))
      .map((region) => region.loopId),
  );

  for (const node of nodes) {
    const nodeId = String(node.id);

    if (node.id === context?.currentNodeId || (available && !available.has(nodeId))) {
      continue;
    }

    if (node.kind === "action") {
      const config = workflowNodeActionConfig(node);
      const provider = providers.find((item) => item.name === config.provider);
      const action = provider?.actions.find((item) => item.function_name === config.action);

      for (const result of action?.results ?? []) {
        collectTypedReferences(
          nodeId,
          [nodeId, result.name],
          [nodeId, result.name],
          result.ty,
          references,
        );
      }

      continue;
    }

    const outputType = findNodeKindMetadata(String(node.kind))?.output_type;

    if (node.kind === "loop" && outputType?.type === "struct") {
      for (const [name, field] of Object.entries(outputType.fields)) {
        if (!scopedLoops.has(nodeId) && !["count", "results"].includes(name)) {continue;}
        collectTypedReferences(nodeId, [nodeId, name], [nodeId, name], field.ty, references);
      }

      continue;
    }

    collectTypedReferences(nodeId, [nodeId], [nodeId], outputType ?? undefined, references);
  }

  return references;
}

function upstreamNodeIds(nodes: JsonRecord[], currentNodeId: string | null): Set<string> | null {
  if (!currentNodeId) {
    return null;
  }

  const reverse = new Map<string, Set<string>>();
  const backEdges = new Set<string>();

  for (const region of loopRegions(nodes)) {
    for (const member of region.nodes) {backEdges.add(`${member}->${region.loopId}`);}
  }

  for (const node of nodes) {
    const source = String(node.id);

    for (const target of nodeTargets(node)) {
      if (backEdges.has(`${source}->${target}`)) {continue;}
      const predecessors = reverse.get(target) ?? new Set<string>();
      predecessors.add(source);
      reverse.set(target, predecessors);
    }
  }

  const upstream = new Set<string>();
  const stack = [...(reverse.get(currentNodeId) ?? [])];

  while (stack.length > 0) {
    const id = stack.pop();
    if (!id || upstream.has(id)) {continue;}
    upstream.add(id);
    stack.push(...(reverse.get(id) ?? []));
  }

  return upstream;
}

function loopRegions(nodes: JsonRecord[]): { loopId: string; nodes: Set<string> }[] {
  const byId = new Map(nodes.map((node) => [String(node.id), node]));
  return nodes
    .filter((node) => node.kind === "loop")
    .flatMap((loop) => {
      const loopId = String(loop.id);
      const transitions = (loop.transitions ?? {}) as JsonRecord;
      const exit = nodeRefId(transitions.on_success);
      const body = nodeRefId(transitions.next);
      if (!body) {return [];}
      const region = new Set<string>();
      const stack = [body];

      while (stack.length > 0) {
        const id = stack.pop();
        if (!id || id === loopId || id === exit || region.has(id)) {continue;}
        region.add(id);
        const node = byId.get(id);
        if (node) {stack.push(...nodeTargets(node));}
      }

      return [{ loopId, nodes: region }];
    });
}

function collectTypedReferences(
  node: string,
  labelPath: string[],
  insertPath: string[],
  ty: RuninatorType | undefined,
  references: (WorkflowReference & { node: string })[],
) {
  if (!ty) {
    return;
  }

  const label = formatLabelPath(labelPath);
  const insert = insertPath.join(".");
  references.push({ node, label, insert, type: describeType(ty) });

  if (ty.type === "struct") {
    for (const [name, field] of Object.entries(ty.fields)) {
      collectTypedReferences(
        node,
        [...labelPath, name],
        [...insertPath, name],
        field.ty,
        references,
      );
    }

    return;
  }

  if (ty.type === "array") {
    collectTypedReferences(node, [...labelPath, "[]"], [...insertPath, "0"], ty.items, references);
  }
}

function formatLabelPath(parts: string[]): string {
  let label = "";

  for (const part of parts) {
    if (part === "[]") {
      label += "[]";
      continue;
    }

    label = label ? `${label}.${part}` : part;
  }

  return label;
}

// the full reference catalog for the picker, grouped by origin. empty groups are dropped.
export function workflowReferenceGroups(
  context?: WorkflowExpressionEditorContext,
): ReferenceGroup[] {
  const groups: ReferenceGroup[] = [];

  const params = paramsReferences(context?.workflowInputType ?? null);

  if (params.length > 0) {
    groups.push({ title: "Workflow parameters", references: params });
  }

  // group prior node outputs under each producing node so the source is obvious.
  const byNode = new Map<string, WorkflowReference[]>();

  for (const ref of nodeOutputReferences(context)) {
    const bucket = byNode.get(ref.node) ?? [];
    bucket.push({ label: ref.label, insert: ref.insert, type: ref.type });
    byNode.set(ref.node, bucket);
  }

  for (const [node, references] of byNode) {
    groups.push({ title: `Output of ${node}`, references });
  }

  groups.push({ title: "Run state", references: STATIC_ROOTS });
  return groups;
}

// build the context a lowered expression resolves against from a run's data, mirroring the
// reducer's runtime context: `params` is the run parameters, `steps.<node>.output` each node's
// output, and `prev` the most recent output. `config`/`secret` are not available client-side, so
// references to them resolve to null in a preview.
export function buildSampleContext(
  detail: WorkflowRunDetail | null | undefined,
): JsonRecord | null {
  if (!detail) {
    return null;
  }

  const steps: JsonRecord = {};
  let prev: JsonValue | null = null;

  for (const node of detail.nodes) {
    if (node.output_json === undefined || node.output_json === null) {
      continue;
    }

    const previous = (steps[node.node_id] ?? {}) as JsonRecord;
    const outputs: JsonValue[] = Array.isArray(previous.outputs)
      ? (previous.outputs as JsonValue[])
      : [];
    steps[node.node_id] = { output: node.output_json, outputs: [...outputs, node.output_json] };
    prev = node.output_json;
  }

  return {
    params: detail.run.parameters ?? {},
    steps,
    prev,
    workflow: {
      run_id: detail.run.id,
      workflow_id: detail.run.workflow_id,
      state: detail.run.status,
    },
  };
}

// a compact, human-readable rendering of a runinator type.
export function describeType(ty: RuninatorType | undefined): string {
  if (!ty) {
    return "any";
  }

  if (ty.type === "array") {
    return `${describeType(ty.items)}[]`;
  }

  if (ty.type === "map") {
    return `map<string, ${describeType(ty.values)}>`;
  }

  if (ty.type === "union") {
    return ty.variants.map(describeType).join(" | ");
  }

  if (ty.type === "enum") {
    return `enum[${ty.values.map((value) => JSON.stringify(value)).join(", ")}]`;
  }

  if (ty.type === "range") {
    return `${describeType(ty.base)} range ${String(ty.min ?? "")}..${String(ty.max ?? "")}`;
  }

  return ty.type;
}
