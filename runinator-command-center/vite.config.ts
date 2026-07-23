import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import tailwindcss from "@tailwindcss/vite";

const WS_DEV_TARGET = process.env.VITE_RUNINATOR_WS_URL ?? "http://127.0.0.1:8080";
const WS_WS_TARGET = WS_DEV_TARGET.replace(/^http/, "ws");

export default defineConfig({
  plugins: [vue(), tailwindcss()],
  clearScreen: false,
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
