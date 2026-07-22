import type { Capability } from "../domain/models";
import type { AppTab, NavSection } from "./app";

export const navSections: NavSection[] = [
  {
    label: "Workspace",
    items: [
      { tab: "Dev", label: "Dev", icon: "debug", desktopOnly: true },
      { tab: "Pipelines", label: "Pipelines", icon: "branch" },
      { tab: "PipelineRuns", label: "Pipeline Runs", icon: "runs" },
      {
        tab: "Workflows",
        label: "Workflows",
        icon: "workflow",
        searchPlaceholder: "Search workflows",
      },
      { tab: "Runs", label: "Runs", icon: "runs", searchPlaceholder: "Search runs" },
      { tab: "Providers", label: "Providers", icon: "box", searchPlaceholder: "Search providers" },
      { tab: "Replicas", label: "Replicas", icon: "list", searchPlaceholder: "Search replicas" },
    ],
  },
  {
    label: "Inbox",
    items: [
      {
        tab: "Approvals",
        label: "Approvals",
        icon: "approve",
        endpoint: "approvals",
        searchPlaceholder: "Search approvals",
      },
      {
        tab: "Notifications",
        label: "Notifications",
        icon: "bell",
        endpoint: "notifications",
        searchPlaceholder: "Search notifications",
      },
    ],
  },
  {
    label: "Data",
    items: [
      {
        tab: "Artifacts",
        label: "Artifacts",
        icon: "box",
        endpoint: "artifacts",
        searchPlaceholder: "Search artifacts",
      },
      {
        tab: "ExternalItems",
        label: "External Items",
        icon: "tag",
        endpoint: "external_items",
        searchPlaceholder: "Search external items",
      },
      {
        tab: "Events",
        label: "Events",
        icon: "flag",
        endpoint: "automation_events",
        searchPlaceholder: "Search events",
      },
    ],
  },
  {
    label: "Other",
    items: [
      { tab: "Gates", label: "Gates", icon: "gate", searchPlaceholder: "Search gates" },
      {
        tab: "Configs",
        label: "Configs",
        icon: "settings",
        requires: "secrets:read",
        searchPlaceholder: "Search configs",
      },
      {
        tab: "Secrets",
        label: "Secrets",
        icon: "key",
        requires: "secrets:read",
        searchPlaceholder: "Search secrets",
      },
    ],
  },
  {
    label: "Organization",
    items: [
      { tab: "Organization", label: "Organization", icon: "shield" },
      { tab: "OrgResources", label: "Resources & Billing", icon: "box" },
    ],
  },
  {
    label: "Admin",
    items: [
      { tab: "AdminSettings", label: "Settings", icon: "settings", requires: "settings:manage" },
      {
        tab: "Permissions",
        label: "Permissions",
        icon: "shield",
        requires: "users:manage",
        searchPlaceholder: "Search users & teams",
      },
      { tab: "DeadLetters", label: "Dead Letters", icon: "flag", requires: "deadletters:read" },
      { tab: "AuditLog", label: "Audit Log", icon: "list", requires: "audit:read" },
    ],
  },
];

export const tabs: AppTab[] = navSections.flatMap((section) =>
  section.items.map((item) => item.tab),
);

const navItemByTab = new Map(
  navSections.flatMap((section) => section.items.map((item) => [item.tab, item] as const)),
);

export function navItemForTab(tab: AppTab) {
  return navItemByTab.get(tab);
}

export function endpointForTab(tab: AppTab): string | undefined {
  return navItemByTab.get(tab)?.endpoint;
}

export function isResourceTab(tab: AppTab): boolean {
  const endpoint = endpointForTab(tab);

  if (!endpoint) {
    return false;
  }

  return endpoint !== "artifacts" && endpoint !== "notifications";
}

export function visibleNavSections(options: {
  can: (capability: Capability) => boolean;
  isDesktop: boolean;
}): NavSection[] {
  const sections = navSections
    .map((section) => ({
      ...section,
      items: section.items.filter((item) => !item.requires || options.can(item.requires)),
    }))
    .filter((section) => section.items.length > 0);

  if (options.isDesktop) {
    return sections;
  }

  return sections
    .map((section) => ({ ...section, items: section.items.filter((item) => !item.desktopOnly) }))
    .filter((section) => section.items.length > 0);
}

export function readStoredDefaultTab(): AppTab {
  try {
    const stored = localStorage.getItem("command-center.defaultTab");

    if (stored && (tabs as string[]).includes(stored)) {
      return stored as AppTab;
    }
  } catch {
    // storage unavailable.
  }

  return "Workflows";
}

export function readSidebarCollapsed(): boolean {
  try {
    return localStorage.getItem("command-center.sidebar.collapsed") === "true";
  } catch {
    return false;
  }
}
