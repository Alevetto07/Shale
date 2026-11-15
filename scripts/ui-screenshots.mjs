#!/usr/bin/env node
/**
 * Headless UI capture with VITE_SHALE_MOCK=1 + vite preview.
 * Usage: node scripts/ui-screenshots.mjs --out docs/screens/after --width 1280
 */
import { spawn } from "node:child_process";
import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";

const root = path.dirname(fileURLToPath(import.meta.url + "/.."));
const desktop = path.join(root, "apps/desktop");
const require = createRequire(path.join(desktop, "package.json"));
const { chromium } = require("playwright");

const shots = [
  { name: "home", url: "/?mock=home&recents=1" },
  { name: "home-empty", url: "/?mock=home-empty&recents=0" },
  { name: "home-about", url: "/?mock=home", click: [{ text: "About and credits", role: "button" }] },
  { name: "home-settings", url: "/?mock=home", click: [{ text: "Settings", role: "button" }] },
  { name: "home-printers", url: "/?mock=home", click: [{ text: "Printers", role: "button" }] },
  { name: "prepare", url: "/?mock=workspace-preview&pane=prepare" },
  { name: "preview", url: "/?mock=workspace-preview&pane=preview" },
  { name: "device-lan", url: "/?mock=device-lan&pane=device" },
  { name: "device-cloud-in", url: "/?mock=device-cloud-in&cloud=in&pane=device" },
  { name: "device-cloud-out", url: "/?mock=device-cloud-out&cloud=out&pane=device" },
  {
    name: "process-sheet",
    url: "/?mock=workspace-preview&pane=prepare",
    click: [{ text: "Slice settings", role: "button" }],
  },
  {
    name: "visibility-sheet",
    url: "/?mock=workspace-preview&pane=prepare",
    click: [
      { text: "Slice settings", role: "button" },
      { text: "settings shown", role: "button" },
    ],
  },
  {
    name: "titlebar-menu",
    url: "/?mock=workspace-preview&pane=prepare",
    click: [{ text: "File", role: "button" }],
  },
];

function arg(name, fallback) {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : fallback;
}

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
  for (let attempt = 0; attempt < 60; attempt += 1) {
    try {
      const response = await fetch(base);
      if (response.ok) return { child, base };
    } catch {
      // still starting
    }
    await new Promise((r) => setTimeout(r, 500));
  }
  child.kill();
  throw new Error("vite preview did not start");
}

async function main() {
  const outDir = path.resolve(root, arg("--out", "docs/screens/after"));
  const width = Number(arg("--width", "1280"));
  const height = Number(arg("--height", "800"));
  await mkdir(outDir, { recursive: true });
  await build();
  const { child, base } = await previewServer();
  const browser = await chromium.launch({
    args: ["--use-angle=swiftshader-webgl", "--enable-webgl", "--ignore-gpu-blocklist"],
  });
  const context = await browser.newContext({
    viewport: { width, height },
    deviceScaleFactor: 1,
  });
  const capture = [];
  for (const shot of shots) {
    const page = await context.newPage();
    await page.goto(base + shot.url, { waitUntil: "networkidle" });
    await page.waitForTimeout(1800);
    if (shot.url.includes("pane=prepare") || shot.url.includes("pane=preview") || shot.url.includes("pane=device")) {
      await page.waitForSelector(".workspace, .device-pane, .viewport-mock", { timeout: 15_000 }).catch(() => {});
    } else {
      await page.waitForSelector(".home-pane, .start", { timeout: 15_000 }).catch(() => {});
    }
    await page.waitForTimeout(400);
    for (const step of shot.click ?? []) {
      const locator = page.getByRole(step.role, { name: step.text, exact: false }).first();
      if (await locator.count()) {
        await locator.click();
        await page.waitForTimeout(600);
      }
    }
    const file = path.join(outDir, `${shot.name}-${width}x${height}.png`);
    await page.screenshot({ path: file, fullPage: false });
    capture.push(file);
    await page.close();
  }
  await browser.close();
  child.kill("SIGTERM");
  const manifest = { width, height, captured_at: new Date().toISOString(), files: capture.map((f) => path.relative(root, f)) };
  await writeFile(path.join(outDir, `manifest-${width}x${height}.json`), JSON.stringify(manifest, null, 2));
  console.log(JSON.stringify(manifest, null, 2));
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});