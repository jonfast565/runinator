import { describe, expect, it } from "vitest";
import { buildSettingsTree, type SettingsTreeFolder } from "../settings-tree";
import type { CredentialSummary } from "../../types/models";

function setting(
  scope: string,
  name: string,
  kind: CredentialSummary["kind"] = "config",
): CredentialSummary {
  return { scope, name, kind };
}

describe("buildSettingsTree", () => {
  it("groups settings by scope into folders with leaves", () => {
    const tree = buildSettingsTree([
      setting("github", "token"),
      setting("github", "webhook_secret"),
      setting("foreign_languages", "python"),
    ]);

    expect(tree.map((node) => node.path)).toEqual(["foreign_languages", "github"]);
    const github = tree.find((node) => node.path === "github") as SettingsTreeFolder;
    expect(github.type).toBe("folder");
    expect(github.children.map((child) => child.label)).toEqual(["token", "webhook_secret"]);
    expect(github.children.every((child) => child.type === "leaf")).toBe(true);
  });

  it("splits dotted names into nested folders", () => {
    const tree = buildSettingsTree([setting("database", "primary.host")]);
    const database = tree[0] as SettingsTreeFolder;
    expect(database.path).toBe("database");
    const primary = database.children[0] as SettingsTreeFolder;
    expect(primary.type).toBe("folder");
    expect(primary.path).toBe("database.primary");
    expect(primary.children[0]).toMatchObject({
      type: "leaf",
      label: "host",
      path: "database.primary.host",
    });
  });

  it("orders folders before leaves at the same level", () => {
    const tree = buildSettingsTree([setting("api", "url"), setting("api", "nested.value")]);
    const api = tree[0] as SettingsTreeFolder;
    expect(api.children.map((child) => child.type)).toEqual(["folder", "leaf"]);
    expect(api.children.map((child) => child.label)).toEqual(["nested", "url"]);
  });

  it("ignores entries with empty paths", () => {
    expect(buildSettingsTree([setting("", "")])).toEqual([]);
  });
});
