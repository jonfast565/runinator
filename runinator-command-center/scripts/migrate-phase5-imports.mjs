#!/usr/bin/env node
/**
 * Phase 5: rewrite legacy shim import paths to canonical core/ + ui/ locations.
 */
import { readFileSync, writeFileSync, readdirSync, statSync } from "node:fs";
import { join, extname } from "node:path";

const ROOT = new URL("../src", import.meta.url).pathname;

const SKIP_DIRS = new Set([
  "node_modules",
  "dist",
  "stores",
  "api",
  "utils",
  "types",
  "composables",
  "components",
]);

const CORE_UTILS = [
  "approvals",
  "format",
  "inputs",
  "json",
  "json-pointer",
  "key-value-object",
  "resources",
  "secrets",
  "settings-tree",
  "status",
  "url-sync",
  "values",
  "wdl-expression",
  "websocket",
  "workflow-references",
  "zip",
];

const CODEMIRROR_UTILS = [
  "codemirror-lang-wdl",
  "codemirror-theme",
  "wdl-completion",
  "wdl-hover",
  "json-completion",
  "workflow-expression-completion",
  "expression-insert-target",
];

function prefixReplacements(prefix, targetPrefix) {
  const rules = [];

  rules.push([new RegExp(`(from ["'])${prefix}stores/`, "g"), `$1${targetPrefix}ui/adapters/pinia/`]);
  rules.push([new RegExp(`(from ["'])${prefix}types/models`, "g"), `$1${targetPrefix}core/domain/models`]);
  rules.push([new RegExp(`(from ["'])${prefix}types/app`, "g"), `$1${targetPrefix}core/navigation/app`]);
  rules.push([new RegExp(`(from ["'])${prefix}types/json`, "g"), `$1${targetPrefix}core/domain/json`]);
  rules.push([new RegExp(`(from ["'])${prefix}types/icons`, "g"), `$1${targetPrefix}core/domain/icons`]);
  rules.push([new RegExp(`(from ["'])${prefix}api/httpRuntime`, "g"), `$1${targetPrefix}core/api/httpRuntime`]);
  rules.push([new RegExp(`(from ["'])${prefix}api/tauriRuntime`, "g"), `$1${targetPrefix}ui/adapters/tauri/runtime`]);
  rules.push([new RegExp(`(from ["'])${prefix}utils/workflows`, "g"), `$1${targetPrefix}core/workflow`]);

  for (const util of CORE_UTILS) {
    rules.push([
      new RegExp(`(from ["'])${prefix}utils/${util}`, "g"),
      `$1${targetPrefix}core/utils/${util}`,
    ]);
  }

  for (const util of CODEMIRROR_UTILS) {
    rules.push([
      new RegExp(`(from ["'])${prefix}utils/${util}`, "g"),
      `$1${targetPrefix}ui/adapters/codemirror/${util}`,
    ]);
  }

  return rules;
}

const REPLACEMENTS = [
  ...prefixReplacements("\\./", "./"),
  ...prefixReplacements("\\.\\./", "../"),
  ...prefixReplacements("\\.\\./\\.\\./", "../../"),
  ...prefixReplacements("\\.\\./\\.\\./\\.\\./", "../../../"),
  ...prefixReplacements("\\.\\./\\.\\./\\.\\./\\.\\./", "../../../../"),
];

function walk(dir, files = []) {
  for (const entry of readdirSync(dir)) {
    const path = join(dir, entry);
    const stat = statSync(path);

    if (stat.isDirectory()) {
      if (SKIP_DIRS.has(entry) && dir === ROOT) {
        continue;
      }

      walk(path, files);
      continue;
    }

    const ext = extname(path);
    if (ext === ".ts" || ext === ".vue") {
      files.push(path);
    }
  }

  return files;
}

let changed = 0;

for (const file of walk(ROOT)) {
  let text = readFileSync(file, "utf8");
  const original = text;

  for (const [pattern, replacement] of REPLACEMENTS) {
    text = text.replace(pattern, replacement);
  }

  if (text !== original) {
    writeFileSync(file, text);
    changed += 1;
    console.log("updated:", file.replace(ROOT + "/", ""));
  }
}

console.log(`\nDone. ${changed} files updated.`);
