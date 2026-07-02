import { defineStore } from "pinia";
import { reactive, ref } from "vue";
import {
  fetchCredentials,
  fetchForeignLanguageRuntime,
  saveForeignLanguageRuntime,
} from "../api/commandCenterApi";
import { useAppStore } from "./app";

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
] as const;

function createLanguageSettings(): ForeignLanguageSetting[] {
  return LANGUAGE_DEFINITIONS.map((definition) => ({
    ...definition,
    aliases: [...definition.aliases],
    image: definition.defaultImage,
    setup_script: "",
  }));
}

export const useAdminSettingsStore = defineStore("adminSettings", () => {
  const app = useAppStore();
  const loaded = ref(false);
  const languages = reactive(createLanguageSettings());

  async function refresh() {
    const settings = await app.runOperation("Loading admin settings", () => fetchCredentials());
    const existing = new Set(
      settings
        .filter(
          (setting) => (setting.kind ?? "secret") === "config" && setting.scope === LANGUAGE_SCOPE,
        )
        .map((setting) => setting.name),
    );

    for (const runtime of languages) {
      runtime.image = runtime.defaultImage;
      runtime.setup_script = "";

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

    loaded.value = true;
  }

  async function saveLanguage(language: string) {
    const runtime = languages.find((entry) => entry.language === language);

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
    runtime.image = image;
    app.setStatus(`${runtime.label} foreign language runtime saved`);
    loaded.value = true;
  }

  function clear() {
    loaded.value = false;
    const fresh = createLanguageSettings();
    languages.splice(0, languages.length, ...fresh);
  }

  return {
    loaded,
    languages,
    refresh,
    saveLanguage,
    clear,
  };
});
