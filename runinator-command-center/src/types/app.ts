export type AppTab =
  | "Dev"
  | "Workflows"
  | "Runs"
  | "Providers"
  | "Replicas"
  | "Approvals"
  | "Artifacts"
  | "Notifications"
  | "Events"
  | "ExternalItems"
  | "Gates"
  | "Configs"
  | "Secrets"
  | "AdminSettings"
  | "Permissions"
  | "DeadLetters"
  | "AuditLog"
  | "Organization"
  | "OrgResources";

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
  // only available to admins, or to auth-disabled stacks where every caller is an admin.
  adminOnly?: boolean;
}

export interface NavSection {
  label: string;
  items: NavItem[];
}
