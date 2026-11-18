import { defineConfig } from "astro/config";
import react from "@astrojs/react";

export default defineConfig({
  integrations: [react()],
  output: "static",
  site: "https://shale.alessiovettorato.com",
  build: {
    inlineStylesheets: "always",
  },
  vite: {
    build: {
      rollupOptions: {
        output: {
          manualChunks(id) {
            if (id.includes("three") || id.includes("@react-three")) return "three";
            if (id.includes("motion")) return "motion";
          },
        },
      },
    },
  },
});