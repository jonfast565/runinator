export type AppTab =
  | "Dev"
  | "Workflows"
  | "Runs"
  | "Providers"
  | "Approvals"
  | "Artifacts"
  | "Notifications"
  | "Feedback"
  | "Events"
  | "ExternalItems"
  | "ChangeSets"
  | "Workspaces"
  | "Gates"
  | "Secrets";

export interface ResourceEndpoint {
  label: string;
  endpoint: string;
}

export interface NavItem {
  tab: AppTab;
  label: string;
  icon: string;
  endpoint?: string;
}

export interface NavSection {
  label: string;
  items: NavItem[];
}
