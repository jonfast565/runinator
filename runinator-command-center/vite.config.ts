import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import tailwindcss from "@tailwindcss/vite";

const WS_DEV_TARGET = process.env.VITE_RUNINATOR_WS_URL ?? "http://127.0.0.1:8080";
const WS_WS_TARGET = WS_DEV_TARGET.replace(/^http/, "ws");

const packageJson = JSON.parse(
  readFileSync(fileURLToPath(new URL("./package.json", import.meta.url)), "utf8"),
) as { version: string };

// the build id identifies *which* build of a version this is. an explicit stamp wins (the docker
// image build has no .git to read), otherwise the local commit; empty when neither is available.
function resolveBuildId(): string {
  const stamped = process.env.RUNINATOR_BUILD_ID?.trim();

  if (stamped) {
    return stamped;
  }

  try {
    return execFileSync("git", ["rev-parse", "--short", "HEAD"], {
      stdio: ["ignore", "pipe", "ignore"],
    })
      .toString()
      .trim();
  } catch {
    return "";
  }
}

export default defineConfig({
  plugins: [vue(), tailwindcss()],
  clearScreen: false,
  // build stamp read back through core/utils/build-info.ts.
  define: {
    __APP_VERSION__: JSON.stringify(packageJson.version),
    __APP_BUILD_ID__: JSON.stringify(resolveBuildId()),
    __APP_BUILD_TIME__: JSON.stringify(new Date().toISOString()),
  },
  build: {
    target: "es2022",
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (
            id.includes("/node_modules/@codemirror/") ||
            id.includes("/node_modules/@lezer/") ||
            id.includes("/node_modules/codemirror/")
          ) {
            return "editor";
          }

          if (id.includes("/node_modules/@vue-flow/")) {
            return "workflow-graph";
          }

          if (
            id.includes("/node_modules/vue/") ||
            id.includes("/node_modules/@vue/") ||
            id.includes("/node_modules/pinia/")
          ) {
            return "vue";
          }
        },
      },
    },
  },
  test: {
    setupFiles: ["./src/test-setup.ts"],
  },
  server: {
    port: 5173,
    strictPort: true,
    proxy: {
      // web-mode browser hits /api/* and /ws/*; the dev server proxies to a
      // local runinator-ws. In prod the nginx pod plays this role.
      "/api": {
        target: WS_DEV_TARGET,
        changeOrigin: true,
        rewrite: (path) => path.replace(/^\/api/, ""),
      },
      "/ws": {
        target: WS_WS_TARGET,
        ws: true,
        changeOrigin: true,
      },
    },
  },
  envPrefix: ["VITE_", "TAURI_"],
});
