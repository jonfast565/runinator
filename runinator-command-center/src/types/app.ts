import type { IconName } from "../types/icons";

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
  icon: IconName;
  endpoint?: string;
  // only available in the tauri desktop client; hidden in the hosted web app.
  desktopOnly?: boolean;
  // only available to admins, or to auth-disabled stacks where every caller is an admin.
  adminOnly?: boolean;
  // placeholder for the global search box; when set the tab's list consumes app.searchQuery.
  // when unset the search box is hidden for this tab so it is never a dead control.
  searchPlaceholder?: string;
}

export interface NavSection {
  label: string;
  items: NavItem[];
}
