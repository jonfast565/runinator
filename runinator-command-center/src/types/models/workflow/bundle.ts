import type { WorkflowDefinition } from "./definition";
import type { WorkflowTrigger } from "./trigger";

export interface WorkflowBundle {
  workflows: WorkflowDefinition[];
  triggers: WorkflowTrigger[];
}
