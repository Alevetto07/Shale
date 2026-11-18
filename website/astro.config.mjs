import { defineConfig } from "astro/config";

export default defineConfig({
  output: "static",
  site: "https://shale.alessiovettorato.com",
  build: {
    inlineStylesheets: "always",
  },
});