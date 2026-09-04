import { describe, expect, it } from "vitest";
import type { Action } from "../../domain/models";
import { navSectionForTab, navSections, visibleNavSections } from "../nav-config";

function labels(sections: ReturnType<typeof visibleNavSections>): string[] {
  return sections.flatMap((section) => section.items.map((item) => item.tab));
}

describe("visibleNavSections", () => {
  it("gives every page prescriptive guidance", () => {
    for (const item of navSections.flatMap((section) => section.items)) {
      expect(item.description.trim(), item.tab).not.toBe("");
    }
  });

  it("hides action-gated tabs when the action is absent", () => {
    const tabs = labels(
      visibleNavSections({ can: () => false, isDesktop: true, isPlatformScope: false }),
    );

    // gated admin/secrets tabs are hidden...
    expect(tabs).not.toContain("AdminSettings");
    expect(tabs).not.toContain("Permissions");
    expect(tabs).not.toContain("AuditLog");
    expect(tabs).not.toContain("DeadLetters");
    expect(tabs).not.toContain("Secrets");
    expect(tabs).not.toContain("Configs");
    // ...while ungated tabs remain.
    expect(tabs).toContain("Workflows");
    expect(tabs).toContain("Runs");
  });

  it("shows a tab exactly when its required action is held", () => {
    const held = new Set<Action>(["audit:read"]);
    const tabs = labels(
      visibleNavSections({
        can: (action) => held.has(action),
        isDesktop: true,
        isPlatformScope: false,
      }),
    );

    expect(tabs).toContain("AuditLog");
    expect(tabs).not.toContain("Permissions");
  });

  it("shows every gated tab when all actions are held (e.g. auth disabled)", () => {
    const tabs = labels(
      visibleNavSections({ can: () => true, isDesktop: true, isPlatformScope: true }),
    );

    expect(tabs).toContain("AdminSettings");
    expect(tabs).toContain("Permissions");
    expect(tabs).toContain("Secrets");
  });

  it("hides platform-only tabs while an organization scope is active", () => {
    const tabs = labels(
      visibleNavSections({ can: () => true, isDesktop: true, isPlatformScope: false }),
    );

    expect(tabs).not.toContain("Permissions");
  });

  it("groups navigation by the operator's workflow", () => {
    const section = (label: string) =>
      navSections.find((item) => item.label === label)?.items ?? [];

    expect(section("Build").map((item) => item.tab)).toEqual([
      "Workflows",
      "Pipelines",
      "Functions",
      "Workspaces",
      "Files",
    ]);
    expect(section("Run & review").map((item) => item.tab)).toEqual([
      "Runs",
      "PipelineRuns",
      "Orchestrations",
      "Approvals",
      "Gates",
    ]);
    expect(section("Integrations").map((item) => item.tab)).toContain("Providers");
    expect(section("Operate").map((item) => item.tab)).toEqual([
      "Console",
      "Replicas",
      "Schedules",
      "Configs",
      "Secrets",
      "Dev",
    ]);
    expect(navSectionForTab("Functions")).toBe("Build");
    expect(navSectionForTab("Notifications")).toBe("Integrations");
    expect(navSectionForTab("Profile")).toBe("Account");
  });
});
