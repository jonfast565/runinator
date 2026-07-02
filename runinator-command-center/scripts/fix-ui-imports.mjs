import { readFileSync, writeFileSync, readdirSync, statSync } from "node:fs";
import { join, relative } from "node:path";

const uiRoot = new URL("../src/ui", import.meta.url).pathname;

function walk(dir, files = []) {
  for (const entry of readdirSync(dir)) {
    const path = join(dir, entry);
    const stat = statSync(path);

    if (stat.isDirectory()) {
      walk(path, files);
    } else if (/\.(vue|ts)$/.test(entry)) {
      files.push(path);
    }
  }

  return files;
}

for (const file of walk(uiRoot)) {
  const rel = relative(uiRoot, file);
  const depth = rel.split("/").length - 1;
  const prefix = "../".repeat(depth + 1);
  let content = readFileSync(file, "utf8");
  let changed = false;

  for (const target of ["api/", "stores/", "types/", "utils/"]) {
    for (let dots = 1; dots <= 4; dots++) {
      const wrongPrefix = "../".repeat(dots);

      if (wrongPrefix === prefix) {
        continue;
      }

      for (const quote of ['"', "'"]) {
        const wrong = `from ${quote}${wrongPrefix}${target}`;

        if (content.includes(wrong)) {
          content = content.split(wrong).join(`from ${quote}${prefix}${target}`);
          changed = true;
        }
      }
    }
  }

  if (changed) {
    writeFileSync(file, content);
    console.log("fixed", rel);
  }
}
