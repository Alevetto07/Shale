import { defineConfig } from "astro/config";
import react from "@astrojs/react";

export default defineConfig({
  integrations: [react()],
  output: "static",
  site: "https://shale.alessiovettorato.com",
  build: {
    inlineStylesheets: "always",
  },
});