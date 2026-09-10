import { defaultApi, type PipelineChainLinksApi } from "../../api/ports/pipeline-chain-links";

import type { WorkflowTrigger } from "../../domain/models";
import type { ChainEvent } from "../../workflow/pipeline-graph";

/** Persist selector/enabled edits to an existing chained trigger (pipeline tag preserved). */
export function createPipelineChainLinksService(api: PipelineChainLinksApi = defaultApi) {
  async function updateChainLink(
    trigger: WorkflowTrigger,
    changes: { on?: ChainEvent; enabled?: boolean },
  ): Promise<WorkflowTrigger> {
    return api.saveWorkflowTrigger(
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

  async function deleteChainLink(triggerId: string): Promise<void> {
    await api.deleteWorkflowTrigger(triggerId);
  }

  return { updateChainLink, deleteChainLink };
}

export const { updateChainLink, deleteChainLink } = createPipelineChainLinksService();
