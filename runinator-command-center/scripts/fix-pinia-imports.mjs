import { readFileSync, writeFileSync, readdirSync, statSync } from "node:fs";
import { join, relative } from "node:path";

const piniaRoot = new URL("../src/ui/adapters/pinia", import.meta.url).pathname;

function walk(dir, files = []) {
  for (const entry of readdirSync(dir)) {
    const path = join(dir, entry);
    const stat = statSync(path);

    if (stat.isDirectory()) {
      walk(path, files);
    } else if (/\.ts$/.test(entry) && !entry.endsWith(".manifest.ts")) {
      files.push(path);
    }
  }

  return files;
}

for (const file of walk(piniaRoot)) {
  const rel = relative(piniaRoot, file);
  const depth = rel.split("/").length - 1;
  const prefix = "../".repeat(depth + 3);
  let content = readFileSync(file, "utf8");
  let changed = false;

  for (const target of ["api/", "stores/", "types/", "utils/", "core/"]) {
    for (let dots = 1; dots <= 5; dots++) {
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
