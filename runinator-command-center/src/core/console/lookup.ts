// resolving the id-or-name references console commands accept.
//
// `runinatorctl` accepts a workflow UUID or name everywhere. The console must do the same, or
// half the commands documented in `:help` would only work if you had an id to hand.

import { fetchPipelines, fetchWorkflows } from "../api/commandCenterApi";
import type { Pipeline, WorkflowDefinition } from "../domain/models";

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

export function isUuid(value: string): boolean {
  return UUID.test(value);
}

/// one workflow by id or name; the newest version wins when a name has several.
export async function resolveWorkflow(reference: string): Promise<WorkflowDefinition> {
  const workflows = await fetchWorkflows();

  if (isUuid(reference)) {
    const byId = workflows.find((workflow) => workflow.id === reference);

    if (byId) {
      return byId;
    }

    throw new Error(`workflow ${reference} not found`);
  }

  const named = workflows.filter((workflow) => workflow.name === reference);

  if (named.length === 0) {
    throw new Error(`workflow '${reference}' not found`);
  }

  return named.reduce((newest, candidate) =>
    compareVersions(candidate.version, newest.version) > 0 ? candidate : newest,
  );
}

// compare two semantic versions numerically, so `1.10.0` sorts above `1.9.0` rather than below it
// the way a string comparison would.
function compareVersions(left: string, right: string): number {
  const parts = (value: string) => value.split(".").map((part) => Number.parseInt(part, 10) || 0);
  const [leftParts, rightParts] = [parts(left), parts(right)];

  for (let index = 0; index < Math.max(leftParts.length, rightParts.length); index += 1) {
    const difference = (leftParts[index] ?? 0) - (rightParts[index] ?? 0);

    if (difference !== 0) {
      return difference;
    }
  }

  return 0;
}

/// a workflow's id, or the failure that says it has none.
export async function resolveWorkflowId(reference: string): Promise<string> {
  const workflow = await resolveWorkflow(reference);

  if (!workflow.id) {
    throw new Error(`workflow '${reference}' has no id`);
  }

  return workflow.id;
}

export async function resolvePipeline(reference: string): Promise<Pipeline> {
  const pipelines = await fetchPipelines();
  const found = pipelines.find(
    (pipeline) => pipeline.id === reference || pipeline.name === reference,
  );

  if (!found) {
    throw new Error(`pipeline '${reference}' not found`);
  }

  return found;
}
