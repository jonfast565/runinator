export type AppTab =
  | "Dev"
  | "Workflows"
  | "Runs"
  | "Providers"
  | "Replicas"
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
  // only available in the tauri desktop client; hidden in the hosted web app.
  desktopOnly?: boolean;
}

export interface NavSection {
  label: string;
  items: NavItem[];
}
