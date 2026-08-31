import {
  fetchCredentials,
  fetchForeignLanguageRuntime,
  fetchAuthSettings,
  fetchServerSettings,
  saveForeignLanguageRuntime,
  saveAuthSettings,
  saveServerSettings,
  type ServerSettingDefinition,
  type RuntimeSettingDefinition,
  type ServerSettingsValues,
} from "../api/commandCenterApi";
import { createStore } from "./event-bus";
import type { AppService } from "./app";

const LANGUAGE_SCOPE = "foreign_languages";

export interface ForeignLanguageSetting {
  language: string;
  label: string;
  aliases: string[];
  defaultImage: string;
  defaultSetupScript?: string;
  defaultExecutable: string;
  image: string;
  setup_script: string;
  environment_text: string;
  executable: string;
  build_args_text: string;
  run_args_text: string;
  memory_mb: number;
  cpu_millis: number;
  pids: number;
  tmpfs_mb: number;
  max_output_bytes: number;
}

const DEFAULT_LIMITS = {
  memory_mb: 2048,
  cpu_millis: 2000,
  pids: 256,
  tmpfs_mb: 512,
  max_output_bytes: 1024 * 1024,
} as const;

const COMMON_LISP_SETUP = `apt-get update
DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends cl-alexandria cl-trivial-gray-streams cl-yason
rm -rf /var/lib/apt/lists/*`;

const COBOL_SETUP = `apt-get update
DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends gnucobol
rm -rf /var/lib/apt/lists/*`;

const C_SETUP = `apt-get update
DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends gcc libc6-dev
rm -rf /var/lib/apt/lists/*`;

const CPP_SETUP = `apt-get update
DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends g++
rm -rf /var/lib/apt/lists/*`;

const FORTRAN_SETUP = `apt-get update
DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends gfortran
rm -rf /var/lib/apt/lists/*`;

const ADA_SETUP = `apt-get update
DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends gnat
rm -rf /var/lib/apt/lists/*`;

const HASKELL_SETUP = `apt-get update
DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends ghc libghc-aeson-dev
rm -rf /var/lib/apt/lists/*`;

const OCAML_SETUP = `apt-get update
DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends ocaml ocaml-findlib libyojson-ocaml-dev
rm -rf /var/lib/apt/lists/*`;

const ERLANG_SETUP = `apt-get update
DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends erlang-nox erlang-jiffy
rm -rf /var/lib/apt/lists/*`;

const LANGUAGE_DEFINITIONS: readonly {
  language: string;
  label: string;
  aliases: readonly string[];
  defaultImage: string;
  defaultSetupScript?: string;
  defaultExecutable: string;
}[] = [
  { language: "python", label: "Python", aliases: ["py"], defaultImage: "python:3.12", defaultExecutable: "python" },
  { language: "javascript", label: "JavaScript", aliases: ["js", "node"], defaultImage: "node:22", defaultExecutable: "node" },
  { language: "bash", label: "Bash", aliases: ["sh"], defaultImage: "bash:5.2", defaultExecutable: "bash" },
  {
    language: "commonlisp",
    label: "Common Lisp",
    aliases: ["common-lisp", "common_lisp", "lisp", "cl", "sbcl"],
    defaultImage: "clfoundation/sbcl:2.6.1-bookworm",
    defaultSetupScript: COMMON_LISP_SETUP,
    defaultExecutable: "sbcl",
  },
  {
    language: "cobol",
    label: "COBOL",
    aliases: ["cob", "gnucobol"],
    defaultImage: "debian:bookworm-slim",
    defaultSetupScript: COBOL_SETUP,
    defaultExecutable: "cobc",
  },
  {
    language: "c",
    label: "C (GCC)",
    aliases: ["gcc", "c17"],
    defaultImage: "debian:bookworm-slim",
    defaultSetupScript: C_SETUP,
    defaultExecutable: "gcc",
  },
  {
    language: "cpp",
    label: "C++ (G++)",
    aliases: ["c++", "cxx", "cplusplus", "g++"],
    defaultImage: "debian:bookworm-slim",
    defaultSetupScript: CPP_SETUP,
    defaultExecutable: "g++",
  },
  {
    language: "fortran",
    label: "Fortran (GFortran)",
    aliases: ["f90", "f95", "gfortran"],
    defaultImage: "debian:bookworm-slim",
    defaultSetupScript: FORTRAN_SETUP,
    defaultExecutable: "gfortran",
  },
  {
    language: "ada",
    label: "Ada (GNAT)",
    aliases: ["adb", "gnat"],
    defaultImage: "debian:bookworm-slim",
    defaultSetupScript: ADA_SETUP,
    defaultExecutable: "gnatmake",
  },
  {
    language: "haskell",
    label: "Haskell (GHC)",
    aliases: ["hs", "ghc"],
    defaultImage: "debian:bookworm-slim",
    defaultSetupScript: HASKELL_SETUP,
    defaultExecutable: "ghc",
  },
  {
    language: "ocaml",
    label: "OCaml",
    aliases: ["ml", "ocamlopt"],
    defaultImage: "debian:bookworm-slim",
    defaultSetupScript: OCAML_SETUP,
    defaultExecutable: "ocamlfind",
  },
  {
    language: "erlang",
    label: "Erlang (escript)",
    aliases: ["erl", "escript"],
    defaultImage: "debian:bookworm-slim",
    defaultSetupScript: ERLANG_SETUP,
    defaultExecutable: "escript",
  },
  { language: "ruby", label: "Ruby", aliases: ["rb"], defaultImage: "ruby:3.3", defaultExecutable: "ruby" },
  { language: "perl", label: "Perl", aliases: ["pl"], defaultImage: "perl:5.40", defaultExecutable: "perl" },
  { language: "php", label: "PHP", aliases: [], defaultImage: "php:8.3-cli", defaultExecutable: "php" },
  { language: "go", label: "Go", aliases: ["golang"], defaultImage: "golang:1.26", defaultExecutable: "go" },
  { language: "swift", label: "Swift", aliases: [], defaultImage: "swift:6.3", defaultExecutable: "swiftc" },
  {
    language: "powershell",
    label: "PowerShell",
    aliases: ["pwsh", "ps1"],
    defaultImage: "mcr.microsoft.com/dotnet/sdk:8.0",
    defaultExecutable: "pwsh",
  },
  {
    language: "csharp",
    label: "C#",
    aliases: ["c#", "cs"],
    defaultImage: "mcr.microsoft.com/dotnet/sdk:10.0",
    defaultExecutable: "dotnet",
  },
  {
    language: "fsharp",
    label: "F#",
    aliases: ["f#", "fs"],
    defaultImage: "mcr.microsoft.com/dotnet/sdk:10.0",
    defaultExecutable: "dotnet",
  },
  {
    language: "vbnet",
    label: "VB.NET",
    aliases: ["vb.net", "visualbasic", "vb"],
    defaultImage: "mcr.microsoft.com/dotnet/sdk:10.0",
    defaultExecutable: "dotnet",
  },
] as const;

export function createLanguageSettings(): ForeignLanguageSetting[] {
  return LANGUAGE_DEFINITIONS.map((definition) => ({
    ...definition,
    aliases: [...definition.aliases],
    image: definition.defaultImage,
    setup_script: definition.defaultSetupScript ?? "",
    environment_text: "",
    executable: definition.defaultExecutable,
    build_args_text: "",
    run_args_text: "",
    ...DEFAULT_LIMITS,
  }));
}

export interface AdminSettingsState {
  loaded: boolean;
  languages: ForeignLanguageSetting[];
  maxRefreshes: number;
  serverValues: ServerSettingsValues;
  serverCatalog: ServerSettingDefinition[];
  runtimeCatalog: RuntimeSettingDefinition[];
}

export function createAdminSettingsService(app: AppService) {
  const store = createStore<AdminSettingsState>({
    loaded: false,
    languages: createLanguageSettings(),
    maxRefreshes: 100,
    serverValues: {},
    serverCatalog: [],
    runtimeCatalog: [],
  });

  const service = {
    ...store,
    updateLanguageField(language: string, field: "image" | "setup_script" | "environment_text" | "executable" | "build_args_text" | "run_args_text", value: string) {
      store.setState((state) => ({
        ...state,
        languages: state.languages.map((runtime) =>
          runtime.language === language ? { ...runtime, [field]: value } : runtime,
        ),
      }));
    },
    updateLanguageLimit(language: string, field: keyof typeof DEFAULT_LIMITS, value: number) {
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
          runtime.setup_script =
            typeof value.setup_script === "string"
              ? value.setup_script
              : (runtime.defaultSetupScript ?? "");

          const environment = value.environment;
          runtime.environment_text = environment && typeof environment === "object"
            ? Object.entries(environment).map(([name, entry]) => `${name}=${entry}`).join("\n")
            : "";

          const toolchain = value.toolchain;
          runtime.executable = typeof toolchain?.executable === "string" && toolchain.executable.trim()
            ? toolchain.executable
            : runtime.defaultExecutable;
          runtime.build_args_text = Array.isArray(toolchain?.build_args) ? toolchain.build_args.join("\n") : "";
          runtime.run_args_text = Array.isArray(toolchain?.run_args) ? toolchain.run_args.join("\n") : "";

          const limits = value.limits;

          for (const [field, fallback] of Object.entries(DEFAULT_LIMITS)) {
            const configured = limits?.[field as keyof typeof DEFAULT_LIMITS];
            runtime[field as keyof typeof DEFAULT_LIMITS] =
              typeof configured === "number" && Number.isInteger(configured) && configured > 0
                ? configured
                : fallback;
          }
        }
      }

      store.setState((state) => ({
        ...state,
        loaded: true,
        languages,
      }));
    },
    async refreshServerSettings() {
      const server = await app.runOperation("Loading server settings", fetchServerSettings);
      store.setState((state) => ({
        ...state,
        serverValues: server.values,
        serverCatalog: server.catalog,
        runtimeCatalog: server.runtime_catalog ?? [],
        maxRefreshes: Number(server.values.authentication.max_refreshes),
      }));
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
    updateServerSetting(key: string, value: number | boolean) {
      const definition = store.getState().serverCatalog.find((item) => item.key === key);

      if (!definition) {
        app.setError(`Unknown server setting: ${key}`);
        return;
      }

      if (definition.kind === "boolean") {
        if (typeof value !== "boolean") {
          app.setError(`${key} must be enabled or disabled`);
          return;
        }
      } else {
        if (typeof value !== "number" || !Number.isInteger(value)) {
          app.setError(`${key} must be an integer`);
          return;
        }

        if (value < definition.minimum || value > definition.maximum) {
          app.setError(
            `${key} must be between ${String(definition.minimum)} and ${String(definition.maximum)}`,
          );
          return;
        }
      }

      const [section, name] = key.split(".");

      store.setState((state) => ({
        ...state,
        serverValues: {
          ...state.serverValues,
          [section]: { ...state.serverValues[section], [name]: value },
        },
      }));
    },
    async saveServerSettings() {
      const saved = await app.runOperation("Saving server settings", () =>
        saveServerSettings(store.getState().serverValues),
      );
      store.setState((state) => ({
        ...state,
        serverValues: saved.values,
        serverCatalog: saved.catalog,
        maxRefreshes: Number(saved.values.authentication.max_refreshes),
      }));
      app.setStatus(
        "Server settings saved; engine and archiver replicas will refresh them shortly",
      );
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

      const executable = runtime.executable.trim();

      if (!executable) {
        app.setError(`${runtime.label} executable is required`);
        return;
      }

      const environment: Record<string, string> = {};

      for (const line of runtime.environment_text.split("\n")) {
        if (!line.trim()) {
          continue;
        }

        const separator = line.indexOf("=");

        if (separator < 1) {
          app.setError(`${runtime.label} environment entries must use NAME=value`);
          return;
        }

        const name = line.slice(0, separator).trim();

        if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(name)) {
          app.setError(`${runtime.label} environment contains invalid variable name ${name}`);
          return;
        }

        if (["RUNINATOR_CONTEXT", "RUNINATOR_OUTPUT", "RUNINATOR_LANGUAGE"].includes(name)) {
          app.setError(`${runtime.label} environment cannot override reserved variable ${name}`);
          return;
        }

        environment[name] = line.slice(separator + 1);
      }

      for (const field of Object.keys(DEFAULT_LIMITS) as (keyof typeof DEFAULT_LIMITS)[]) {
        if (!Number.isInteger(runtime[field]) || runtime[field] <= 0) {
          app.setError(`${runtime.label} ${field} must be a positive integer`);
          return;
        }
      }

      await app.runOperation(`Saving ${runtime.label} runtime`, () =>
        saveForeignLanguageRuntime(runtime.language, {
          image,
          setup_script: runtime.setup_script,
          environment,
          toolchain: {
            executable,
            build_args: runtime.build_args_text.split("\n").filter((value) => value.length > 0),
            run_args: runtime.run_args_text.split("\n").filter((value) => value.length > 0),
          },
          limits: {
            memory_mb: runtime.memory_mb,
            cpu_millis: runtime.cpu_millis,
            pids: runtime.pids,
            tmpfs_mb: runtime.tmpfs_mb,
            max_output_bytes: runtime.max_output_bytes,
          },
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
        serverValues: {},
        serverCatalog: [],
        runtimeCatalog: [],
      }));
    },
  };

  return service;
}

export type AdminSettingsService = ReturnType<typeof createAdminSettingsService>;
