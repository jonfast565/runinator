import { mkdir } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const commandCenter = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const workspace = resolve(commandCenter, "..");
const output = resolve(commandCenter, "src/core/console/wasm");
const built = resolve(workspace, "target/wasm32-unknown-unknown/release/runinator_ctl_wasm.wasm");

const cargo = spawnSync(
  "cargo",
  ["build", "--locked", "--release", "--target", "wasm32-unknown-unknown", "-p", "runinator-ctl-wasm"],
  { cwd: workspace, stdio: "inherit" },
);

if (cargo.status !== 0) {
  process.exit(cargo.status ?? 1);
}

await mkdir(output, { recursive: true });
const bindgen = spawnSync(
  "wasm-bindgen",
  [built, "--target", "web", "--out-dir", output, "--out-name", "runinator_ctl_wasm"],
  { cwd: workspace, stdio: "inherit" },
);

if (bindgen.status !== 0) {
  process.exit(bindgen.status ?? 1);
}
