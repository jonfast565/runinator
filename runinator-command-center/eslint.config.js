import js from "@eslint/js";
import tseslint from "typescript-eslint";
import pluginVue from "eslint-plugin-vue";
import vueParser from "vue-eslint-parser";
import prettier from "eslint-config-prettier";
import globals from "globals";

// the type-checked presets are strict. the stylistic-but-noisy rules stay as
// warnings for visibility, but the type-safety core (no `any`, no unsafe access)
// is promoted back to hard errors in the house-rules block below.
const typeCheckedPresets = [
  ...tseslint.configs.strictTypeChecked,
  ...tseslint.configs.stylisticTypeChecked,
];

const mergedTypeCheckedRules = {};

for (const config of typeCheckedPresets) {
  if (config.rules) {
    Object.assign(mergedTypeCheckedRules, config.rules);
  }
}

// downgrade every enabled type-checked rule to a warning; leave disabled ones off.
const typeCheckedAsWarnings = {};

for (const [name, setting] of Object.entries(mergedTypeCheckedRules)) {
  const severity = Array.isArray(setting) ? setting[0] : setting;

  if (severity === "off" || severity === 0) {
    continue;
  }

  typeCheckedAsWarnings[name] = "warn";
}

// strict lint config for the command center. type-checked rules require a
// project service so .ts and .vue <script> blocks are analyzed with type info.
export default tseslint.config(
  {
    ignores: [
      "dist/**",
      "node_modules/**",
      "src-tauri/**",
      "public/**",
      "coverage/**",
      "scripts/**",
      "*.config.*",
      "*.cjs",
    ],
  },

  js.configs.recommended,
  ...typeCheckedPresets,
  ...pluginVue.configs["flat/recommended"],

  {
    languageOptions: {
      ecmaVersion: 2022,
      sourceType: "module",
      globals: { ...globals.browser, ...globals.es2022 },
      parserOptions: {
        projectService: true,
        tsconfigRootDir: import.meta.dirname,
        extraFileExtensions: [".vue"],
      },
    },
  },

  {
    files: ["**/*.vue"],
    languageOptions: {
      parser: vueParser,
      parserOptions: {
        parser: tseslint.parser,
        projectService: true,
        tsconfigRootDir: import.meta.dirname,
        extraFileExtensions: [".vue"],
      },
    },
  },

  // shared primitives are intentionally single-word design-system components
  // (Button, Icon, Modal, Sparkline); the multi-word rule doesn't apply to them.
  {
    files: ["src/ui/components/shared/**/*.vue", "src/components/shared/**/*.vue"],
    rules: {
      "vue/multi-word-component-names": "off",
    },
  },

  // prettier must come after everything else to disable formatting rules that
  // would otherwise fight the formatter.
  prettier,

  {
    files: ["src/core/**/*.ts"],
    rules: {
      "no-restricted-imports": [
        "error",
        {
          patterns: [
            {
              group: ["vue", "pinia", "@vue-flow/*", "@codemirror/*", "@tauri-apps/*"],
              message: "core/ must not import Vue, Pinia, Vue Flow, CodeMirror, or Tauri.",
            },
            {
              group: ["**/ui/**"],
              message: "core/ must not import from ui/.",
            },
          ],
        },
      ],
    },
  },

  {
    files: ["src/core/**/__tests__/**/*.ts"],
    rules: {
      "no-restricted-imports": "off",
    },
  },

  // downgrade the type-checked family to warnings (visibility, not a gate).
  {
    rules: typeCheckedAsWarnings,
  },

  // house rules kept as hard errors on top of the presets.
  {
    rules: {
      // no `any` and no unsafe access to `any` values anywhere in source. the
      // domain model is fully typed (JsonRecord = Record<string, unknown>); the
      // test-only override below relaxes these where fixtures poke at internals.
      "@typescript-eslint/no-explicit-any": "error",
      "@typescript-eslint/no-unsafe-argument": "error",
      "@typescript-eslint/no-unsafe-assignment": "error",
      "@typescript-eslint/no-unsafe-call": "error",
      "@typescript-eslint/no-unsafe-member-access": "error",
      "@typescript-eslint/no-unsafe-return": "error",

      // rule 1: every if/else/for/while must use braces, even single-line.
      curly: ["error", "all"],

      // rule 2: keep blank-line separation between braced constructs and the
      // statements around them. a variable declared at the top of a block and
      // used by that block's body is not a "block vs variable" boundary, so it
      // is intentionally allowed to bump straight into the body.
      "padding-line-between-statements": [
        "error",
        { blankLine: "always", prev: "*", next: "multiline-block-like" },
        { blankLine: "always", prev: "multiline-block-like", next: "*" },
        { blankLine: "always", prev: "*", next: "function" },
        { blankLine: "always", prev: "function", next: "*" },
        { blankLine: "always", prev: "*", next: "class" },
        { blankLine: "always", prev: "class", next: "*" },
      ],
    },
  },

  // tests legitimately poke at dynamic internals and mock shapes; relax the
  // any/unsafe family (and non-null assertions) so fixtures stay terse.
  {
    files: ["**/*.test.ts", "**/__tests__/**"],
    rules: {
      "@typescript-eslint/no-explicit-any": "off",
      "@typescript-eslint/no-unsafe-argument": "off",
      "@typescript-eslint/no-unsafe-assignment": "off",
      "@typescript-eslint/no-unsafe-call": "off",
      "@typescript-eslint/no-unsafe-member-access": "off",
      "@typescript-eslint/no-unsafe-return": "off",
      "@typescript-eslint/no-non-null-assertion": "off",
    },
  },
);
