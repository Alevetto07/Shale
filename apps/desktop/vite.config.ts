/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test-setup.ts"],
    env: { VITE_SHALE_MOCK: "1" },
    css: true,
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: "127.0.0.1",
  },
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    target: "es2022",
    sourcemap: false,
    // The plate is a dynamic import. Preloading it would parse three.js before
    // the shell has painted.
    modulePreload: {
      resolveDependencies(_filename, deps) {
        return deps.filter(
          (dep) => !/Viewport-|DevicePane-|SettingsView-|SetupWizard-|three/.test(dep),
        );
      },
    },
  },
});
