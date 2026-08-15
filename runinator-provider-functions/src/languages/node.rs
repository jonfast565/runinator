//! the node runtime adapter.

use super::RuntimeAdapter;

pub struct Node;

impl RuntimeAdapter for Node {
    fn family(&self) -> &'static str {
        "node"
    }

    fn default_image(&self, version: &str) -> String {
        let version = if version.is_empty() { "22" } else { version };
        format!("node:{version}-slim")
    }

    fn shim_filename(&self) -> &'static str {
        "_runinator_shim.mjs"
    }

    fn shim_source(&self) -> &'static str {
        SHIM
    }

    fn command(&self, runtime_dir: &str) -> Vec<String> {
        vec!["node".into(), format!("{runtime_dir}/_runinator_shim.mjs")]
    }
}

// the handler is dotted in the manifest whatever the runtime, so it reads the same across
// languages; here the last segment is the export and the rest is the module path.
const SHIM: &str = r#"// loads and calls one packaged-function export.
import { readFile, writeFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";
import path from "node:path";

async function main() {
  const packagePath = process.env.RUNINATOR_PACKAGE;
  const handler = process.env.RUNINATOR_HANDLER;
  const inputPath = process.env.RUNINATOR_INPUT;
  const outputPath = process.env.RUNINATOR_OUTPUT;

  const dot = handler.lastIndexOf(".");
  if (dot <= 0) {
    throw new Error(`handler must be '<module>.<function>', got ${handler}`);
  }
  const modulePath = handler.slice(0, dot).split(".").join(path.sep);
  const exportName = handler.slice(dot + 1);

  const payload = JSON.parse(await readFile(inputPath, "utf8"));
  const args = payload.input ?? {};
  if (typeof args !== "object" || Array.isArray(args)) {
    throw new Error("input must be an object");
  }

  // resolved against the package root and imported as a file url, so the module graph is the
  // package's own and nothing outside the mount is reachable by a relative specifier.
  const resolved = await importFirst(packagePath, modulePath);
  const fn = resolved[exportName];
  if (typeof fn !== "function") {
    throw new Error(`module ${modulePath} has no export ${exportName}`);
  }

  const result = await fn(args);
  await writeFile(outputPath, JSON.stringify(result ?? null), "utf8");
}

// a package may ship either extension; trying both keeps the manifest from having to say which.
async function importFirst(packagePath, modulePath) {
  const candidates = [".mjs", ".js", "/index.mjs", "/index.js"];
  let lastError;
  for (const suffix of candidates) {
    try {
      const full = path.join(packagePath, modulePath + suffix);
      return await import(pathToFileURL(full).href);
    } catch (error) {
      lastError = error;
    }
  }
  throw lastError ?? new Error(`cannot resolve module ${modulePath}`);
}

main().catch((error) => {
  // stderr is what the runner captures as the failure detail; the exit code is what it branches on.
  console.error(error?.stack ?? String(error));
  process.exit(1);
});
"#;
