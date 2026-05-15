export type AppTab = "Tasks" | "Workflows" | "Runs" | "Resources" | "Secrets";

export interface ResourceEndpoint {
  label: string;
  endpoint: string;
}
