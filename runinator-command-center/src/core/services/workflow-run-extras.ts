import {
  deliverSignal,
  fetchWorkflowEffectOutput,
  fetchWorkflowRunArtifacts,
  settleWorkflowEffect,
} from "../api/commandCenterApi";
import type {
  JsonValue,
  RunArtifact,
  TerminalInteraction,
  WorkflowEffectOutputEvent,
  WorkflowRunArtifact,
} from "../domain/models";
import type { AppService } from "./app";

export function createWorkflowRunExtrasService(app: AppService) {
  return {
    fetchNodeRunArtifacts(effectId: string) {
      return app
        .runOperation("Loading workflow effect artifacts", async () =>
          (await fetchWorkflowEffectOutput(effectId))
            .filter((event) => event.output.type === "artifact")
            .map((event) => {
              const artifact = event.output.type === "artifact" ? event.output.artifact : {};
              return {
                ...(artifact as Record<string, unknown>),
                id: event.event_id,
                run_id: event.workflow_run_id,
                workflow_node_run_id: null,
                created_at: new Date(event.created_at * 1000).toISOString(),
              } as RunArtifact;
            }),
        )
        .catch(() => [] as RunArtifact[]);
    },
    fetchRunArtifacts(runId: string) {
      return app
        .runOperation("Loading run artifacts", () => fetchWorkflowRunArtifacts(runId))
        .catch(() => [] as WorkflowRunArtifact[]);
    },
    fetchNodeRunChunks(effectId: string) {
      return app.runOperation("Loading workflow effect log", async () =>
        (await fetchWorkflowEffectOutput(effectId))
          .filter((event) => event.output.type === "chunk")
          .map((event) => ({
            id: event.event_id,
            effect_id: event.effect_id,
            continuation_id: event.continuation_id,
            run_id: event.workflow_run_id,
            workflow_node_run_id: null,
            stream: event.output.type === "chunk" ? event.output.stream : "log",
            content: event.output.type === "chunk" ? event.output.content : "",
            attempt: event.attempt,
            created_at: new Date(event.created_at * 1000).toISOString(),
          })),
      );
    },
    fetchTerminalInteraction(effectId: string) {
      return app
        .runOperation("Loading terminal interaction", async () => {
          return activeTerminalInteraction(await fetchWorkflowEffectOutput(effectId));
        })
        .catch(() => null as TerminalInteraction | null);
    },
    deliverSignal(workflowRunId: string, name: string, payload: unknown = {}) {
      return app.runOperation(`Sending signal '${name}'`, () =>
        deliverSignal(workflowRunId, name, payload),
      );
    },
    resolveInput(
      effectId: string,
      outputJson: unknown,
      resolvedBy?: string,
      message?: string,
    ) {
      return app.runOperation("Resolving workflow input", () =>
        settleWorkflowEffect(effectId, "succeeded", outputJson as JsonValue, message ?? null),
      );
    },
  };
}

export function activeTerminalInteraction(
  events: WorkflowEffectOutputEvent[],
): TerminalInteraction | null {
  const interactions = events
    .filter((event) => event.output.type === "terminal_interaction")
    .sort((left, right) => {
      const leftSequence =
        left.output.type === "terminal_interaction" ? left.output.interaction.sequence : 0;
      const rightSequence =
        right.output.type === "terminal_interaction" ? right.output.interaction.sequence : 0;
      return left.attempt - right.attempt || leftSequence - rightSequence;
    });
  const latest = interactions.at(-1)?.output;

  if (latest?.type !== "terminal_interaction") {
    return null;
  }

  return latest.interaction.state === "input_required" ? latest.interaction : null;
}

export type WorkflowRunExtrasService = ReturnType<typeof createWorkflowRunExtrasService>;
