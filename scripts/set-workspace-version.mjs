#!/usr/bin/env node

import { execFileSync } from "node:child_process";
import { appendFileSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const workspaceRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const cargoManifestPath = resolve(workspaceRoot, "Cargo.toml");
const cargoManifest = readFileSync(cargoManifestPath, "utf8");
const workspacePackage = cargoManifest.match(
  /\[workspace\.package\][\s\S]*?^version\s*=\s*"(\d+)\.(\d+)\.(\d+)"/m,
);

if (!workspacePackage) {
  throw new Error("Cargo.toml does not declare a numeric [workspace.package] version");
}

const commitCount = execFileSync("git", ["rev-list", "--count", "HEAD"], {
  cwd: workspaceRoot,
  encoding: "utf8",
}).trim();

if (!/^\d+$/.test(commitCount) || commitCount === "0") {
  throw new Error(`git returned an invalid commit count: ${commitCount}`);
}

const version = `${workspacePackage[1]}.${workspacePackage[2]}.${commitCount}`;

// major and minor are release decisions; the full-history commit count is the monotonic build.

function replace(path, pattern, replacement) {
  const current = readFileSync(path, "utf8");
  const updated = current.replace(pattern, replacement);
  if (updated === current && !pattern.test(current)) {
    throw new Error(`could not find the version field in ${path}`);
  }
  writeFileSync(path, updated);
}

replace(
  cargoManifestPath,
  /(\[workspace\.package\][\s\S]*?^version\s*=\s*")[^"]+(".*$)/m,
  `$1${version}$2`,
);

for (const relativePath of [
  "runinator-command-center/package.json",
  "runinator-command-center/src-tauri/tauri.conf.json",
  "runinator-lsp/editors/vscode/package.json",
]) {
  replace(
    resolve(workspaceRoot, relativePath),
    /(\"version\"\s*:\s*\")[^\"]+(\")/,
    `$1${version}$2`,
  );
}

for (const relativePath of ["deploy/Dockerfile", "runinator-command-center/Dockerfile"]) {
  replace(
    resolve(workspaceRoot, relativePath),
    /^ARG RUNINATOR_VERSION=.*$/m,
    `ARG RUNINATOR_VERSION=${version}`,
  );
}

replace(
  resolve(workspaceRoot, "scripts/package-macos-backend-apps.sh"),
  /version = "\$\{RUNINATOR_VERSION:-[^}]+\}"/,
  `version = "\${RUNINATOR_VERSION:-${version}}"`,
);

if (process.env.GITHUB_ENV) {
  appendFileSync(process.env.GITHUB_ENV, `RUNINATOR_VERSION=${version}\n`);
}
if (process.env.GITHUB_OUTPUT) {
  appendFileSync(process.env.GITHUB_OUTPUT, `version=${version}\n`);
}

console.log(version);
