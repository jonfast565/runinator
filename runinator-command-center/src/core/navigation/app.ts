import type { IconName } from "../domain/icons";
import type { Capability } from "../domain/models";

export type AppTab =
  | "Dev"
  | "Pipelines"
  | "PipelineRuns"
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
  // capability the caller must hold for this tab to be visible. absent means visible to any
  // authenticated caller. auth-disabled stacks hold every capability, so nothing is hidden there.
  requires?: Capability;
  // placeholder for the global search box; when set the tab's list consumes app.searchQuery.
  // when unset the search box is hidden for this tab so it is never a dead control.
  searchPlaceholder?: string;
}

export interface NavSection {
  label: string;
  items: NavItem[];
}
