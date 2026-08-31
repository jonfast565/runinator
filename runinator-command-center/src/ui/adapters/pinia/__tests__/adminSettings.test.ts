import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { navSections } from "../app";
import { useAdminSettingsStore } from "../adminSettings";

vi.mock("../../../../core/api/commandCenterApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../../core/api/commandCenterApi")>()),
  fetchCredentials: vi.fn(),
  fetchForeignLanguageRuntime: vi.fn(),
  saveForeignLanguageRuntime: vi.fn(),
  fetchServerSettings: vi.fn(),
  saveServerSettings: vi.fn(),
}));

import {
  fetchCredentials,
  fetchForeignLanguageRuntime,
  fetchServerSettings,
  saveForeignLanguageRuntime,
  saveServerSettings,
} from "../../../../core/api/commandCenterApi";

describe("admin settings store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
    vi.stubGlobal("window", {
      clearTimeout: vi.fn(),
      setTimeout: vi.fn(),
    });
    vi.mocked(fetchCredentials).mockResolvedValue([]);
    vi.mocked(fetchForeignLanguageRuntime).mockResolvedValue({
      scope: "foreign_languages",
      name: "python",
      kind: "config",
      value: { image: "python:3.13", setup_script: "pip install requests" },
    });
    vi.mocked(saveForeignLanguageRuntime).mockResolvedValue({
      success: true,
      message: "saved",
    });
    vi.mocked(fetchServerSettings).mockResolvedValue({
      values: { authentication: { max_refreshes: 100 } },
      catalog: [
        {
          key: "authentication.max_refreshes",
          section: "Authentication",
          label: "Maximum refreshes",
          description: "Maximum rotations allowed for one login session.",
          unit: "refreshes",
          default: 100,
          minimum: 1,
          maximum: 100000,
          usual_minimum: 10,
          usual_maximum: 1000,
        },
      ],
    });
    vi.mocked(saveServerSettings).mockImplementation(async (values) => ({
      values,
      catalog: await fetchServerSettings().then((response) => response.catalog),
    }));
  });

  it("shows settings under the admin left nav section", () => {
    const admin = navSections.find((section) => section.label === "Admin");

    expect(admin?.items).toContainEqual({
      tab: "AdminSettings",
      label: "Settings",
      icon: "settings",
      description: "Change server settings carefully and validate language/runtime paths first.",
      requires: "credentials:manage",
    });
  });

  it("loads all default foreign language runtimes when no overrides exist", async () => {
    const settings = useAdminSettingsStore();

    await settings.refresh();

    expect(settings.languages.map((runtime) => [runtime.language, runtime.image])).toEqual([
      ["python", "python:3.12"],
      ["javascript", "node:22"],
      ["bash", "bash:5.2"],
      ["commonlisp", "clfoundation/sbcl:2.6.1-bookworm"],
      ["cobol", "debian:bookworm-slim"],
      ["ruby", "ruby:3.3"],
      ["perl", "perl:5.40"],
      ["php", "php:8.3-cli"],
      ["go", "golang:1.26"],
      ["swift", "swift:6.3"],
      ["powershell", "mcr.microsoft.com/dotnet/sdk:8.0"],
      ["csharp", "mcr.microsoft.com/dotnet/sdk:10.0"],
      ["fsharp", "mcr.microsoft.com/dotnet/sdk:10.0"],
      ["vbnet", "mcr.microsoft.com/dotnet/sdk:10.0"],
    ]);
    expect(
      settings.languages.find((runtime) => runtime.language === "commonlisp")?.setup_script,
    ).toContain("cl-yason");
    expect(
      settings.languages.find((runtime) => runtime.language === "cobol")?.setup_script,
    ).toContain("gnucobol");
    expect(fetchForeignLanguageRuntime).not.toHaveBeenCalled();
  });

  it("loads and saves per-language foreign runtime overrides", async () => {
    vi.mocked(fetchCredentials).mockResolvedValue([
      {
        scope: "foreign_languages",
        name: "python",
        kind: "config",
      },
    ]);
    const settings = useAdminSettingsStore();

    await settings.refresh();
    const python = settings.languages.find((runtime) => runtime.language === "python");
    expect(python?.image).toBe("python:3.13");
    expect(python?.setup_script).toBe("pip install requests");

    if (!python) {
      throw new Error("missing python runtime");
    }

    python.image = "python:3.13-slim";
    await settings.saveLanguage("python");

    expect(saveForeignLanguageRuntime).toHaveBeenCalledWith("python", {
      image: "python:3.13-slim",
      setup_script: "pip install requests",
    });
  });

  it("loads, validates, and saves catalog-driven server settings", async () => {
    const settings = useAdminSettingsStore();

    await settings.refreshServerSettings();
    settings.updateServerSetting("authentication.max_refreshes", 250);
    await settings.saveServerSettings();

    expect(settings.serverCatalog).toHaveLength(1);
    expect(settings.serverValues.authentication.max_refreshes).toBe(250);
    expect(saveServerSettings).toHaveBeenCalledWith({
      authentication: { max_refreshes: 250 },
    });
  });

  it("updates boolean archiver settings without coercing them to numbers", async () => {
    vi.mocked(fetchServerSettings).mockResolvedValue({
      values: {
        authentication: { max_refreshes: 100 },
        archiver: { dry_run: false },
      },
      catalog: [
        {
          key: "archiver.dry_run",
          section: "Archiver",
          label: "Dry run",
          description: "Discover eligible rows without deleting them.",
          unit: "",
          kind: "boolean",
          default: 0,
          minimum: 0,
          maximum: 1,
          usual_minimum: 0,
          usual_maximum: 1,
        },
      ],
    });
    const settings = useAdminSettingsStore();

    await settings.refreshServerSettings();
    settings.updateServerSetting("archiver.dry_run", true);
    await settings.saveServerSettings();

    expect(settings.serverValues.archiver.dry_run).toBe(true);
    expect(saveServerSettings).toHaveBeenCalledWith({
      authentication: { max_refreshes: 100 },
      archiver: { dry_run: true },
    });
  });
});
