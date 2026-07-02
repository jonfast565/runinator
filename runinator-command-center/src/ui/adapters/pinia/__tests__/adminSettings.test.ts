import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { navSections } from "../app";
import { useAdminSettingsStore } from "../adminSettings";

vi.mock("../../../../core/api/commandCenterApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../../core/api/commandCenterApi")>()),
  fetchCredentials: vi.fn(),
  fetchForeignLanguageRuntime: vi.fn(),
  saveForeignLanguageRuntime: vi.fn(),
}));

import {
  fetchCredentials,
  fetchForeignLanguageRuntime,
  saveForeignLanguageRuntime,
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
  });

  it("shows settings under the admin left nav section", () => {
    const admin = navSections.find((section) => section.label === "Admin");

    expect(admin?.items).toContainEqual({
      tab: "AdminSettings",
      label: "Settings",
      icon: "settings",
      adminOnly: true,
    });
  });

  it("loads all default foreign language runtimes when no overrides exist", async () => {
    const settings = useAdminSettingsStore();

    await settings.refresh();

    expect(settings.languages.map((runtime) => [runtime.language, runtime.image])).toEqual([
      ["python", "python:3.12"],
      ["javascript", "node:22"],
      ["bash", "bash:5.2"],
      ["ruby", "ruby:3.3"],
      ["perl", "perl:5.40"],
      ["php", "php:8.3-cli"],
    ]);
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
});
