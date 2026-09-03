import type { IconName } from "../domain/icons";
import type { Action } from "../domain/models";

export type AppTab =
  | "Profile"
  | "Dev"
  | "Pipelines"
  | "PipelineRuns"
  | "Orchestrations"
  | "Workflows"
  | "Runs"
  | "Providers"
  | "Functions"
  | "Files"
  | "ExecutionProfiles"
  | "Console"
  | "Replicas"
  | "Approvals"
  | "Notifications"
  | "Events"
  | "ExternalItems"
  | "Gates"
  | "Schedules"
  | "Configs"
  | "Secrets"
  | "AdminSettings"
  | "Permissions"
  | "IngressControl"
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
  // short, action-oriented guidance shown below the active page title. Every page supplies this so
  // operators never land on an unexplained surface.
  description: string;
  endpoint?: string;
  // only available in the tauri desktop client; hidden in the hosted web app.
  desktopOnly?: boolean;
  // action the caller must hold for this tab to be visible. absent means visible to any
  // authenticated caller. auth-disabled stacks hold every action, so nothing is hidden there.
  requires?: Action;
  // placeholder for the global search box; when set the tab's list consumes app.searchQuery.
  // when unset the search box is hidden for this tab so it is never a dead control.
  searchPlaceholder?: string;
}

export interface NavSection {
  label: string;
  items: NavItem[];
}
