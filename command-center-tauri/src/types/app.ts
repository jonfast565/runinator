export type AppTab = "Tasks" | "Runs" | "Workflows" | "Resources" | "Secrets";

export interface ResourceEndpoint {
  label: string;
  endpoint: string;
}
