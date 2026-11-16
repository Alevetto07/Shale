#!/usr/bin/env node
/** Capture home plates only for nebula + lunar at 1280 and 1920. */
import { spawn } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";

const root = path.dirname(fileURLToPath(import.meta.url + "/.."));
const desktop = path.join(root, "apps/desktop");
const require = createRequire(path.join(desktop, "package.json"));
const { chromium } = require("playwright");
const outDir = path.join(root, "docs/screens/ui-styles");

const shots = [
  { theme: "nebula", slug: "nebula", w: 1280, h: 800 },
  { theme: "lunar", slug: "lunar", w: 1280, h: 800 },
  { theme: "nebula", slug: "nebula", w: 1920, h: 1080, suffix: "home-1080p" },
  { theme: "lunar", slug: "lunar", w: 1920, h: 1080, suffix: "home-1080p" },
];

async function build() {
  await new Promise((resolve, reject) => {
    const child = spawn("npm", ["run", "build"], {
      cwd: desktop,
      env: { ...process.env, VITE_SHALE_MOCK: "1" },
      stdio: "inherit",
    });
    child.on("exit", (code) => (code === 0 ? resolve() : reject(new Error("build failed"))));
  });
}

async function previewServer() {
  const child = spawn("npx", ["vite", "preview", "--host", "127.0.0.1", "--port", "4173", "--strictPort"], {
    cwd: desktop,
    env: { ...process.env, VITE_SHALE_MOCK: "1" },
    stdio: "pipe",
  });
  const base = "http://127.0.0.1:4173";
  for (let i = 0; i < 40; i++) {
    try {
      if ((await fetch(base)).ok) return { child, base };
    } catch {
      /* wait */
    }
    await new Promise((r) => setTimeout(r, 500));
  }
  child.kill();
  throw new Error("preview failed");
}

async function main() {
  await build();
  const { child, base } = await previewServer();
  const browser = await chromium.launch({
    args: ["--use-angle=swiftshader-webgl", "--enable-webgl", "--ignore-gpu-blocklist"],
  });
  for (const shot of shots) {
    const name = shot.suffix ?? "home";
    const file = `${shot.slug}-${name}-${shot.w}x${shot.h}.png`;
    const page = await browser.newPage({
      viewport: { width: shot.w, height: shot.h },
    });
    await page.goto(`${base}/?mock=home&recents=1&uiStyle=${shot.theme}`, { waitUntil: "networkidle" });
    await page.waitForSelector(".start", { timeout: 15000 });
    await page.waitForTimeout(2200);
    await page.screenshot({ path: path.join(outDir, file) });
    await page.close();
    console.log(file);
  }
  await browser.close();
  child.kill("SIGTERM");
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});