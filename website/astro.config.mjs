import { defineConfig } from "astro/config";
import react from "@astrojs/react";

export default defineConfig({
  integrations: [react()],
  output: "static",
  site: "https://shale.example",
  build: {
    inlineStylesheets: "always",
  },
});