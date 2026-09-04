import type { Action } from "../domain/models";
import type { AppTab, NavItem, NavSection } from "./app";

export const navSections: NavSection[] = [
  {
    label: "Build",
    items: [
      {
        tab: "Workflows",
        label: "Workflows",
        icon: "workflow",
        description: "Create or select a workflow, resolve diagnostics, save, then run it.",
        searchPlaceholder: "Search workflows",
      },
      {
        tab: "Pipelines",
        label: "Pipelines",
        icon: "branch",
        description: "Create a pipeline, add workflows, connect hand-offs, then start a run.",
        searchPlaceholder: "Search pipelines",
      },
      {
        tab: "Functions",
        label: "Functions",
        icon: "box",
        description: "Publish a built package, verify exports, then promote an alias deliberately.",
        searchPlaceholder: "Search function packages",
      },
      {
        tab: "Workspaces",
        label: "Workspaces",
        icon: "folder",
        description: "Inspect saved workspace versions, files, and results shared across workflows.",
        searchPlaceholder: "Search workspace keys",
      },
      {
        tab: "Files",
        label: "Files",
        icon: "folder",
        description: "Upload reusable files and pin immutable revisions from workflow inputs.",
        searchPlaceholder: "Search files",
      },
    ],
  },
  {
    label: "Run & review",
    items: [
      {
        tab: "Runs",
        label: "Workflow Runs",
        icon: "runs",
        description: "Select a run to inspect its timeline, outputs, logs, and recovery actions.",
        searchPlaceholder: "Search runs",
      },
      {
        tab: "PipelineRuns",
        label: "Pipeline Runs",
        icon: "runs",
        description: "Start a pipeline run, then inspect each member attempt and hand-off.",
      },
      {
        tab: "Orchestrations",
        label: "Orchestrations",
        icon: "branch",
        description: "Inspect correlated work or configure the adapter that admits it.",
      },
      {
        tab: "Approvals",
        label: "Approvals",
        icon: "approve",
        description: "Review the request and its run context before approving or rejecting it.",
        endpoint: "approvals",
        searchPlaceholder: "Search approvals",
      },
      {
        tab: "Gates",
        label: "Gates",
        icon: "gate",
        description: "Select a blocking gate, verify its run, and record why you open or close it.",
        searchPlaceholder: "Search gates",
      },
    ],
  },
  {
    label: "Integrations",
    items: [
      {
        tab: "Providers",
        label: "Providers",
        icon: "box",
        description: "Verify registered providers, their actions, and required credential scopes.",
        searchPlaceholder: "Search providers",
      },
      {
        tab: "ExecutionProfiles",
        label: "Execution Profiles",
        icon: "key",
        description: "Configure and monitor file-backed identities materialized for provider actions.",
        requires: "credentials:manage",
        searchPlaceholder: "Search execution profiles",
      },
      {
        tab: "ExternalItems",
        label: "External Items",
        icon: "tag",
        description: "Inspect provider-owned records and their linked workflow context.",
        endpoint: "external_items",
        searchPlaceholder: "Search external items",
      },
      {
        tab: "Events",
        label: "Events",
        icon: "flag",
        description: "Trace automation events by provider, run, node, or message.",
        endpoint: "automation_events",
        searchPlaceholder: "Search events",
      },
      {
        tab: "Notifications",
        label: "Notifications",
        icon: "bell",
        description:
          "Review unread alerts and define policies for failures that require attention.",
        endpoint: "notifications",
        searchPlaceholder: "Search notifications",
      },
    ],
  },
  {
    label: "Operate",
    items: [
      {
        // gated: a console cell can start a workflow run, so this is a privilege rather than a view.
        tab: "Console",
        label: "Console",
        icon: "debug",
        description:
          "Run a command, review its output, and keep long-lived work in a saved session.",
        requires: "console:use",
      },
      {
        tab: "Replicas",
        label: "Replicas",
        icon: "list",
        description:
          "Check runtime health first; use drain or restart only on the selected replica.",
        searchPlaceholder: "Search replicas",
      },
      {
        tab: "Schedules",
        label: "Schedules",
        icon: "clock",
        description: "Create a freeze window with a valid time range and the narrowest safe scope.",
        requires: "schedules:manage",
        searchPlaceholder: "Search freeze windows",
      },
      {
        tab: "Configs",
        label: "Configs",
        icon: "settings",
        description: "Store visible JSON configuration and reuse it through scoped references.",
        requires: "secrets:read",
        searchPlaceholder: "Search configs",
      },
      {
        tab: "Secrets",
        label: "Secrets",
        icon: "key",
        description: "Add or rotate a scoped secret; saved values remain write-only.",
        requires: "secrets:read",
        searchPlaceholder: "Search secrets",
      },
      {
        tab: "Dev",
        label: "Dev",
        icon: "debug",
        description: "Start and inspect the local development stack before testing changes.",
        desktopOnly: true,
      },
    ],
  },
  {
    label: "Organization",
    items: [
      {
        tab: "Organization",
        label: "Organization",
        icon: "shield",
        description: "Manage the active organization, members, roles, and teams.",
      },
      {
        tab: "OrgResources",
        label: "Resources & Billing",
        icon: "box",
        description: "Review limits and usage before changing plan or resource allocations.",
      },
    ],
  },
  {
    label: "Administration",
    items: [
      {
        tab: "AdminSettings",
        label: "Settings",
        icon: "settings",
        description: "Change server settings carefully and validate language/runtime paths first.",
        requires: "credentials:manage",
      },
      {
        tab: "Permissions",
        label: "Permissions",
        icon: "shield",
        description: "Grant the minimum required access, then verify users, teams, and API keys.",
        requires: "members:manage",
        requiresPlatformScope: true,
        searchPlaceholder: "Search users & teams",
      },
      {
        tab: "IngressControl",
        label: "Ingress Control",
        icon: "flag",
        description: "Observe, hold, approve, or drop scoped ingress before it reaches orchestration.",
      },
      {
        tab: "AuditLog",
        label: "Audit Log",
        icon: "list",
        description:
          "Filter by actor, action, or resource to reconstruct an administrative change.",
        requires: "audit:read",
      },
    ],
  },
];

const auxiliaryItems: NavItem[] = [
  {
    tab: "Profile",
    label: "Profile & security",
    icon: "lock",
    description: "Manage your account details, signed-in sessions, and personal API keys.",
  },
];

export const tabs: AppTab[] = [
  ...navSections.flatMap((section) => section.items.map((item) => item.tab)),
  ...auxiliaryItems.map((item) => item.tab),
];

const navItemByTab = new Map(
  [...navSections.flatMap((section) => section.items), ...auxiliaryItems].map(
    (item) => [item.tab, item] as const,
  ),
);

const navSectionByTab = new Map(
  navSections.flatMap((section) => section.items.map((item) => [item.tab, section.label] as const)),
);

export function navItemForTab(tab: AppTab) {
  return navItemByTab.get(tab);
}

export function navSectionForTab(tab: AppTab): string {
  return navSectionByTab.get(tab) ?? "Account";
}

export function endpointForTab(tab: AppTab): string | undefined {
  return navItemByTab.get(tab)?.endpoint;
}

export function isResourceTab(tab: AppTab): boolean {
  const endpoint = endpointForTab(tab);

  if (!endpoint) {
    return false;
  }

  return endpoint !== "notifications";
}

export function visibleNavSections(options: {
  can: (action: Action) => boolean;
  isDesktop: boolean;
  isPlatformScope: boolean;
}): NavSection[] {
  const sections = navSections
    .map((section) => ({
      ...section,
      items: section.items.filter(
        (item) =>
          (!item.requires || options.can(item.requires)) &&
          (!item.requiresPlatformScope || options.isPlatformScope),
      ),
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
