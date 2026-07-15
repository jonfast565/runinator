import { describe, expect, it } from "vitest";
import type { Capability } from "../../domain/models";
import { visibleNavSections } from "../nav-config";

function labels(sections: ReturnType<typeof visibleNavSections>): string[] {
  return sections.flatMap((section) => section.items.map((item) => item.tab));
}

describe("visibleNavSections", () => {
  it("hides capability-gated tabs when the capability is absent", () => {
    const tabs = labels(visibleNavSections({ can: () => false, isDesktop: true }));

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

  it("shows a tab exactly when its required capability is held", () => {
    const held = new Set<Capability>(["audit:read"]);
    const tabs = labels(
      visibleNavSections({ can: (capability) => held.has(capability), isDesktop: true }),
    );

    expect(tabs).toContain("AuditLog");
    expect(tabs).not.toContain("Permissions");
  });

  it("shows every gated tab when all capabilities are held (e.g. auth disabled)", () => {
    const tabs = labels(visibleNavSections({ can: () => true, isDesktop: true }));

    expect(tabs).toContain("AdminSettings");
    expect(tabs).toContain("Permissions");
    expect(tabs).toContain("Secrets");
  });
});
