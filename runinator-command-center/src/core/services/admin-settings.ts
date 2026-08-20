import {
  fetchCredentials,
  fetchForeignLanguageRuntime,
  fetchAuthSettings,
  saveForeignLanguageRuntime,
  saveAuthSettings,
} from "../api/commandCenterApi";
import { createStore } from "./event-bus";
import type { AppService } from "./app";

const LANGUAGE_SCOPE = "foreign_languages";

export interface ForeignLanguageSetting {
  language: string;
  label: string;
  aliases: string[];
  defaultImage: string;
  image: string;
  setup_script: string;
}

const LANGUAGE_DEFINITIONS = [
  { language: "python", label: "Python", aliases: ["py"], defaultImage: "python:3.12" },
  { language: "javascript", label: "JavaScript", aliases: ["js", "node"], defaultImage: "node:22" },
  { language: "bash", label: "Bash", aliases: ["sh"], defaultImage: "bash:5.2" },
  { language: "ruby", label: "Ruby", aliases: ["rb"], defaultImage: "ruby:3.3" },
  { language: "perl", label: "Perl", aliases: ["pl"], defaultImage: "perl:5.40" },
  { language: "php", label: "PHP", aliases: [], defaultImage: "php:8.3-cli" },
  { language: "go", label: "Go", aliases: ["golang"], defaultImage: "golang:1.26" },
  { language: "swift", label: "Swift", aliases: [], defaultImage: "swift:6.3" },
  {
    language: "powershell",
    label: "PowerShell",
    aliases: ["pwsh", "ps1"],
    defaultImage: "mcr.microsoft.com/dotnet/sdk:8.0",
  },
  {
    language: "csharp",
    label: "C#",
    aliases: ["c#", "cs"],
    defaultImage: "mcr.microsoft.com/dotnet/sdk:10.0",
  },
  {
    language: "fsharp",
    label: "F#",
    aliases: ["f#", "fs"],
    defaultImage: "mcr.microsoft.com/dotnet/sdk:10.0",
  },
  {
    language: "vbnet",
    label: "VB.NET",
    aliases: ["vb.net", "visualbasic", "vb"],
    defaultImage: "mcr.microsoft.com/dotnet/sdk:10.0",
  },
] as const;

export function createLanguageSettings(): ForeignLanguageSetting[] {
  return LANGUAGE_DEFINITIONS.map((definition) => ({
    ...definition,
    aliases: [...definition.aliases],
    image: definition.defaultImage,
    setup_script: "",
  }));
}

export interface AdminSettingsState {
  loaded: boolean;
  languages: ForeignLanguageSetting[];
  maxRefreshes: number;
}

export function createAdminSettingsService(app: AppService) {
  const store = createStore<AdminSettingsState>({
    loaded: false,
    languages: createLanguageSettings(),
    maxRefreshes: 100,
  });

  const service = {
    ...store,
    updateLanguageField(language: string, field: "image" | "setup_script", value: string) {
      store.setState((state) => ({
        ...state,
        languages: state.languages.map((runtime) =>
          runtime.language === language ? { ...runtime, [field]: value } : runtime,
        ),
      }));
    },
    async refresh() {
      const settings = await app.runOperation("Loading admin settings", () => fetchCredentials());
      const existing = new Set(
        settings
          .filter(
            (setting) =>
              (setting.kind ?? "secret") === "config" && setting.scope === LANGUAGE_SCOPE,
          )
          .map((setting) => setting.name),
      );

      const languages = createLanguageSettings();

      for (const runtime of languages) {
        if (!existing.has(runtime.language)) {
          continue;
        }

        const detail = await app.runOperation(`Loading ${runtime.label} runtime`, () =>
          fetchForeignLanguageRuntime(runtime.language),
        );
        const value = detail.value;

        if (value && typeof value === "object") {
          runtime.image =
            typeof value.image === "string" && value.image.trim()
              ? value.image
              : runtime.defaultImage;
          runtime.setup_script = typeof value.setup_script === "string" ? value.setup_script : "";
        }
      }

      store.setState((state) => ({ ...state, loaded: true, languages }));
    },
    async refreshAuthSettings() {
      const settings = await app.runOperation("Loading authentication settings", fetchAuthSettings);
      store.setState((state) => ({ ...state, maxRefreshes: settings.max_refreshes }));
    },
    updateMaxRefreshes(value: number) {
      if (!Number.isInteger(value) || value < 1 || value > 100000) {
        app.setError("Maximum refreshes must be an integer between 1 and 100000");
        return;
      }
      store.setState((state) => ({ ...state, maxRefreshes: value }));
    },
    async saveAuthSettings() {
      const saved = await app.runOperation("Saving authentication settings", () =>
        saveAuthSettings(store.getState().maxRefreshes),
      );
      store.setState((state) => ({ ...state, maxRefreshes: saved.max_refreshes }));
      app.setStatus("Authentication settings saved");
    },
    async saveLanguage(language: string) {
      const runtime = store.getState().languages.find((entry) => entry.language === language);

      if (!runtime) {
        app.setError(`Unknown foreign language: ${language}`);
        return;
      }

      const image = runtime.image.trim();

      if (!image) {
        app.setError(`${runtime.label} Docker image is required`);
        return;
      }

      await app.runOperation(`Saving ${runtime.label} runtime`, () =>
        saveForeignLanguageRuntime(runtime.language, {
          image,
          setup_script: runtime.setup_script,
        }),
      );

      store.setState((state) => ({
        ...state,
        languages: state.languages.map((entry) =>
          entry.language === language ? { ...entry, image } : entry,
        ),
      }));
      app.setStatus(`${runtime.label} foreign language runtime saved`);
    },
    clear() {
      store.setState(() => ({
        loaded: false,
        languages: createLanguageSettings(),
        maxRefreshes: 100,
      }));
    },
  };

  return service;
}

export type AdminSettingsService = ReturnType<typeof createAdminSettingsService>;
