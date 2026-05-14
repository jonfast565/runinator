export type AppTab = "Workflows" | "Runs" | "Resources" | "Secrets";

export interface ResourceEndpoint {
  label: string;
  endpoint: string;
}
