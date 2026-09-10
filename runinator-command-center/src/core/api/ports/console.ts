import type { ConsoleFunctionsApi } from "./console-functions";
import type { ConsoleInfrastructureApi } from "./console-infrastructure";
import type { ConsoleOperationsApi } from "./console-operations";
import type { ConsoleRexrapApi } from "./console-rexrap";
import type { ConsoleRunsApi } from "./console-runs";
import type { ConsoleSessionApi } from "./console-session";
import type { ConsoleSettingsApi } from "./console-settings";
import type { ConsoleTriggersApi } from "./console-triggers";
import type { ConsoleWorkflowsApi } from "./console-workflows";
import type { ConsoleWorkspacesApi } from "./console-workspaces";
export { defaultApi } from "./default";
export type ConsoleApi = ConsoleFunctionsApi &
  ConsoleInfrastructureApi &
  ConsoleOperationsApi &
  ConsoleRexrapApi &
  ConsoleRunsApi &
  ConsoleSessionApi &
  ConsoleSettingsApi &
  ConsoleTriggersApi &
  ConsoleWorkflowsApi &
  ConsoleWorkspacesApi;
