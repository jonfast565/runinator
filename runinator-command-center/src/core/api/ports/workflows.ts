import type { WorkflowsCatalogApi } from "./workflows-catalog";
import type { WorkflowsEditorApi } from "./workflows-editor";
import type { WorkflowsRunsApi } from "./workflows-runs";
export { defaultApi } from "./default";
export type WorkflowsApi = WorkflowsCatalogApi & WorkflowsEditorApi & WorkflowsRunsApi;
