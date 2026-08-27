import { deleteWorkflowTrigger, saveWorkflowTrigger } from "../../api/commandCenterApi";
import type { WorkflowTrigger } from "../../domain/models";
import type { ChainEvent } from "../../workflow/pipeline-graph";

/** Persist selector/enabled edits to an existing chained trigger (pipeline tag preserved). */
export async function updateChainLink(
  trigger: WorkflowTrigger,
  changes: { on?: ChainEvent; enabled?: boolean },
): Promise<WorkflowTrigger> {
  return saveWorkflowTrigger(
    {
      ...trigger,
      enabled: changes.enabled ?? trigger.enabled,
      configuration: {
        ...trigger.configuration,
        on: changes.on ?? trigger.configuration.on,
      },
    },
    false,
  );
}

export async function deleteChainLink(triggerId: string): Promise<void> {
  await deleteWorkflowTrigger(triggerId);
}
