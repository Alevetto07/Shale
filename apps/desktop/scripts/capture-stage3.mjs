import { chromium } from "playwright";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { mkdirSync } from "node:fs";

const base = process.env.SHALE_CAPTURE_URL ?? "http://127.0.0.1:5173";
const here = path.dirname(fileURLToPath(import.meta.url));
const outDir = path.resolve(here, "../review-screens");
mkdirSync(outDir, { recursive: true });

async function openPalette(page) {
  await page.keyboard.press("Control+K");
  await page.waitForSelector(".ui-command-palette", { timeout: 10_000 });
  await page.waitForTimeout(550);
}

async function capturePalette(page, fileName, query) {
  await page.goto(`${base}/?mock=workspace-prepare&pane=prepare${query}`, {
    waitUntil: "networkidle",
    timeout: 60_000,
  });
  await page.waitForTimeout(1200);
  await openPalette(page);
  await page.screenshot({ path: `${outDir}/${fileName}.png` });
  await page.keyboard.press("Escape");
}

async function captureNonPlanar(page, fileName, query = "") {
  await page.goto(`${base}/?mock=workspace-prepare&pane=prepare${query}`, {
    waitUntil: "networkidle",
    timeout: 60_000,
  });
  await page.waitForTimeout(1200);
  await page.getByRole("button", { name: "Slice settings" }).click({ timeout: 8000 });
  await page.waitForTimeout(600);
  await page.getByRole("tab", { name: "Non-planar" }).click();
  await page.waitForTimeout(500);
  await page.screenshot({ path: `${outDir}/${fileName}.png` });
}

async function captureToastUndo(page, fileName, query = "") {
  await page.goto(`${base}/?mock=workspace-prepare&pane=prepare&models=1${query}`, {
    waitUntil: "networkidle",
    timeout: 60_000,
  });
  await page.waitForTimeout(1200);
  await openPalette(page);
  await page.locator(".ui-command-palette [cmdk-input]").fill("clear plate");
  await page.waitForTimeout(200);
  await page.getByRole("option", { name: /Clear plate/i }).click();
  await page.waitForTimeout(1200);
  const toast = page.locator("[data-sonner-toast].shale-toast");
  await toast.waitFor({ timeout: 5000 });
  await page.screenshot({ path: `${outDir}/${fileName}.png` });
}

async function recordWalkthrough(browser) {
  const context = await browser.newContext({
    viewport: { width: 1440, height: 900 },
    recordVideo: { dir: outDir, size: { width: 1440, height: 900 } },
  });
  const page = await context.newPage();
  await page.goto(`${base}/?mock=workspace-prepare&pane=prepare&models=1`, {
    waitUntil: "networkidle",
    timeout: 60_000,
  });
  await page.waitForTimeout(1200);
  await page.keyboard.press("Control+K");
  await page.waitForSelector(".ui-command-palette", { timeout: 10_000 });
  await page.waitForTimeout(900);
  await page.locator(".ui-command-palette [cmdk-input]").fill("navigate");
  await page.waitForTimeout(600);
  await page.keyboard.press("Escape");
  await page.waitForTimeout(400);
  await openPalette(page);
  await page.locator(".ui-command-palette [cmdk-input]").fill("clear plate");
  await page.waitForTimeout(300);
  await page.getByRole("option", { name: /Clear plate/i }).click();
  await page.waitForTimeout(800);
  await page.getByRole("button", { name: "Undo" }).click({ timeout: 5000 });
  await page.waitForTimeout(1200);
  await page.getByRole("button", { name: "Slice settings" }).click({ timeout: 8000 });
  await page.getByRole("tab", { name: "Non-planar" }).click();
  await page.waitForTimeout(1400);
  await page.keyboard.press("Escape");
  await page.waitForTimeout(400);
  await openPalette(page);
  await page.locator(".ui-command-palette [cmdk-input]").fill("theme");
  await page.getByRole("option", { name: /Toggle theme/i }).click();
  await page.waitForTimeout(1000);
  await page.keyboard.press("Control+K");
  await page.waitForTimeout(900);
  await page.keyboard.press("Escape");
  await page.waitForTimeout(1800);
  await page.keyboard.press("Control+K");
  await page.waitForTimeout(1200);
  await page.keyboard.press("Escape");
  await page.waitForTimeout(1500);
  const video = page.video();
  await context.close();
  const target = `${outDir}/ui-rework-stage3-walkthrough.webm`;
  if (video) await video.saveAs(target);
}

const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
await capturePalette(page, "command-palette-andromeda", "");
await capturePalette(page, "command-palette-lunar", "&uiStyle=lunar");
await captureNonPlanar(page, "non-planar-panel-andromeda", "");
await captureNonPlanar(page, "non-planar-panel-lunar", "&uiStyle=lunar");
await captureToastUndo(page, "toast-undo-clear-plate");
await captureToastUndo(page, "toast-undo-clear-plate-lunar", "&uiStyle=lunar");
await recordWalkthrough(browser);
await browser.close();
console.log(`Stage 3 review assets in ${outDir}`);