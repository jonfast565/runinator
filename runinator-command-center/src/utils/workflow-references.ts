import type { JsonRecord, ProviderMetadata, RuninatorType, WorkflowRunDetail } from "../types/models";
import { workflowNodeActionConfig } from "./workflows";

// the data an expression editor needs to enumerate the references in scope at a given node, plus an
// optional sample context (a prior run's data) the editor can resolve expressions against.
export interface WorkflowExpressionEditorContext {
  workflowInputType?: RuninatorType | null;
  nodes?: JsonRecord[];
  currentNodeId?: string | null;
  providers?: ProviderMetadata[];
  sampleContext?: JsonRecord | null;
}

// a single insertable reference: what to show, the WDL text to splice in, and its declared type.
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
  { label: "secret", insert: "secret", type: "secret reference" }
];

/// references for every field of the workflow parameter struct, flattened by dotted path.
export function paramsReferences(ty: RuninatorType | null | undefined): WorkflowReference[] {
  if (!ty || ty.type !== "struct") return [];
  const references: WorkflowReference[] = [];
  collectParamFields(ty, ["params"], references);
  return references;
}

function collectParamFields(ty: RuninatorType, path: string[], references: WorkflowReference[]) {
  if (ty.type !== "struct") return;
  for (const [name, field] of Object.entries(ty.fields)) {
    const nextPath = [...path, name];
    const dotted = nextPath.join(".");
    references.push({ label: dotted, insert: dotted, type: describeType(field.ty) });
    collectParamFields(field.ty, nextPath, references);
  }
}

/// references for the declared outputs of every prior action node (the current node is excluded).
export function nodeOutputReferences(context?: WorkflowExpressionEditorContext): Array<WorkflowReference & { node: string }> {
  const nodes = context?.nodes ?? [];
  const providers = context?.providers ?? [];
  const references: Array<WorkflowReference & { node: string }> = [];
  for (const node of nodes) {
    if (node.kind !== "action" || node.id === context?.currentNodeId) continue;
    const config = workflowNodeActionConfig(node);
    const provider = providers.find((item) => item.name === config.provider);
    const action = provider?.actions.find((item) => item.function_name === config.action);
    for (const result of action?.results ?? []) {
      const dotted = `${String(node.id)}.${result.name}`;
      references.push({ node: String(node.id), label: dotted, insert: dotted, type: describeType(result.ty) });
    }
  }
  return references;
}

/// the full reference catalog for the picker, grouped by origin. empty groups are dropped.
export function workflowReferenceGroups(context?: WorkflowExpressionEditorContext): ReferenceGroup[] {
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

/// build the context a lowered expression resolves against from a run's data, mirroring the
/// reducer's runtime context: `params` is the run parameters, `steps.<node>.output` each node's
/// output, and `prev` the most recent output. `config`/`secret` are not available client-side, so
/// references to them resolve to null in a preview.
export function buildSampleContext(detail: WorkflowRunDetail | null | undefined): JsonRecord | null {
  if (!detail) return null;
  const steps: JsonRecord = {};
  let prev: unknown = null;
  for (const node of detail.nodes) {
    if (node.output_json === undefined || node.output_json === null) continue;
    steps[node.node_id] = { output: node.output_json };
    prev = node.output_json;
  }
  return {
    params: detail.run.parameters ?? {},
    steps,
    prev,
    workflow: {
      run_id: detail.run.id,
      workflow_id: detail.run.workflow_id ?? null,
      state: detail.run.status
    }
  };
}

/// a compact, human-readable rendering of a runinator type.
export function describeType(ty: RuninatorType | undefined): string {
  if (!ty) return "any";
  if (ty.type === "array") return `${describeType(ty.items)}[]`;
  if (ty.type === "map") return `map<string, ${describeType(ty.values)}>`;
  if (ty.type === "union") return ty.variants.map(describeType).join(" | ");
  return ty.type;
}
