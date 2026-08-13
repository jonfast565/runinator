import type { ProviderMetadata, WorkflowDefinition, WorkflowRunDetail, WorkflowTrigger } from "../../../../core/domain/models";

export const WORKFLOW_ID = "00000000-0000-0000-0000-000000000007";
export const RUN_ID = "00000000-0000-0000-0000-000000000070";
export const NODE_RUN_ID = "00000000-0000-0000-0000-000000000071";
export const TRIGGER_ID = "00000000-0000-0000-0000-000000000012";

export function workflowDefinition(id: string, name: string): WorkflowDefinition {
  return {
    id,
    name,
    version: "1.0.0",
    enabled: true,
    input_type: { type: "struct", fields: {} },
    definition: {
      start: "start",
      nodes: [
        { id: "start", kind: "start", transitions: { next: { $node: "end" } } },
        { id: "end", kind: "end" },
        { id: "fail", kind: "fail" },
      ],
    },
  };
}

export async function flushWorkflowSync() {
  await new Promise((resolve) => setTimeout(resolve, 0));
  await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await Promise.resolve();
}

export function graphCentroid(nodes: { position?: { x: number; y: number } }[]): { x: number; y: number } {
  const positioned = nodes
    .map((node) => ({ x: Number(node.position?.x), y: Number(node.position?.y) }))
    .filter((position) => Number.isFinite(position.x) && Number.isFinite(position.y));
  const totals = positioned.reduce(
    (sum, position) => ({ x: sum.x + position.x, y: sum.y + position.y }),
    { x: 0, y: 0 },
  );
  return {
    x: Math.round(totals.x / positioned.length),
    y: Math.round(totals.y / positioned.length),
  };
}

export function workflowTrigger(id: string, workflowId: string, cron: string): WorkflowTrigger {
  return {
    id,
    workflow_id: workflowId,
    kind: "cron",
    enabled: true,
    configuration: { cron, parameters: {} },
    next_execution: null,
    blackout_start: null,
    blackout_end: null,
    metadata: {},
  };
}

export function workflowDetail(
  id: string,
  status: string,
  message: string,
  breakpoints: string[] = [],
): WorkflowRunDetail {
  return {
    run: {
      id,
      workflow_id: WORKFLOW_ID,
      status,
      parameters: {},
      active_node_id: null,
      created_at: "2026-01-01T00:00:00Z",
      started_at: null,
      finished_at: null,
      message,
    },
    nodes: [],
    execution_state: { debug: { enabled: true, breakpoints } },
  };
}

export function waitingGateWorkflowDetail(): WorkflowRunDetail {
  return {
    run: {
      id: RUN_ID,
      workflow_id: WORKFLOW_ID,
      status: "waiting",
      parameters: {},
      active_node_id: "gate-1",
      created_at: "2026-01-01T00:00:00Z",
      started_at: null,
      finished_at: null,
      message: "waiting on gate",
      workflow_snapshot: {
        id: WORKFLOW_ID,
        name: "gate flow",
        version: "1.0.0",
        enabled: true,
        input_type: { type: "struct", fields: {} },
        definition: {
          start: "start",
          nodes: [
            { id: "start", kind: "start", transitions: { next: { $node: "gate-1" } } },
            {
              id: "gate-1",
              kind: "gate",
              parameters: { kind: "manual", label: "Deploy window" },
              transitions: { next: { $node: "end" } },
            },
            { id: "end", kind: "end" },
            { id: "fail", kind: "fail" },
          ],
        },
      },
    },
    nodes: [
      {
        id: NODE_RUN_ID,
        workflow_run_id: RUN_ID,
        node_id: "gate-1",
        status: "waiting",
        attempt: 1,
        parameters: {},
        state: { gate_id: "gate-1", poll_interval: 30 },
        message: "waiting",
      },
    ],
    execution_state: {},
  };
}

export function nestedWorkflowInputProvider(): ProviderMetadata {
  return {
    name: "workflow-input",
    metadata: { credential_scopes: [], contract: null },
    actions: [
      {
        function_name: "prepare",
        description: null,
        results: [],
        parameters: [
          {
            name: "workflow_input",
            label: "Workflow Input",
            description: null,
            required: true,
            secret: false,
            ty: {
              type: "struct",
              fields: {
                target: { required: true, ty: { type: "string" } },
                environments: {
                  required: true,
                  ty: {
                    type: "map",
                    values: {
                      type: "struct",
                      fields: {
                        url: { required: true, ty: { type: "string" } },
                        retries: { required: false, ty: { type: "integer" } },
                      },
                    },
                  },
                },
                strategy: {
                  required: true,
                  ty: {
                    type: "union",
                    variants: [
                      { type: "string" },
                      {
                        type: "struct",
                        fields: {
                          manual: { required: true, ty: { type: "boolean" } },
                        },
                      },
                    ],
                  },
                },
              },
            },
          },
        ],
      },
    ],
  };
}

export function untypedActionProvider(): ProviderMetadata {
  return {
    name: "webhook",
    metadata: { credential_scopes: [], contract: null },
    actions: [
      {
        function_name: "send",
        description: null,
        results: [],
        parameters: [],
      },
    ],
  };
}
