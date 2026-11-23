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
  await page.waitForTimeout(400);
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

async function captureNonPlanar(page, fileName) {
  await page.goto(`${base}/?mock=workspace-prepare&pane=prepare`, { waitUntil: "networkidle", timeout: 60_000 });
  await page.waitForTimeout(1200);
  await page.getByRole("button", { name: "Slice settings" }).click({ timeout: 8000 });
  await page.waitForTimeout(600);
  await page.getByRole("tab", { name: "Non-planar" }).click();
  await page.waitForTimeout(500);
  await page.screenshot({ path: `${outDir}/${fileName}.png` });
}

async function captureToastUndo(page, fileName) {
  await page.goto(`${base}/?mock=workspace-prepare&pane=prepare`, { waitUntil: "networkidle", timeout: 60_000 });
  await page.waitForTimeout(1200);
  await openPalette(page);
  await page.locator(".ui-command-palette [cmdk-input]").fill("clear plate");
  await page.waitForTimeout(200);
  await page.getByRole("option", { name: /Clear plate/i }).click();
  await page.waitForTimeout(900);
  await page.screenshot({ path: `${outDir}/${fileName}.png` });
}

async function recordWalkthrough(browser) {
  const context = await browser.newContext({
    viewport: { width: 1440, height: 900 },
    recordVideo: { dir: outDir, size: { width: 1440, height: 900 } },
  });
  const page = await context.newPage();
  await page.goto(`${base}/?mock=workspace-prepare&pane=prepare`, { waitUntil: "networkidle", timeout: 60_000 });
  await page.waitForTimeout(1500);
  await page.locator(".ui-sidebar__header-actions button[aria-label='Command palette']").click();
  await page.waitForTimeout(700);
  await page.locator(".ui-command-palette [cmdk-input]").fill("preview");
  await page.waitForTimeout(400);
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  await page.waitForTimeout(800);
  await page.keyboard.press("Control+K");
  await page.waitForTimeout(500);
  await page.locator(".ui-command-palette [cmdk-input]").fill("lunar");
  await page.waitForTimeout(400);
  await page.getByRole("option", { name: /Toggle theme/i }).click();
  await page.waitForTimeout(900);
  await page.getByRole("button", { name: "Slice settings" }).click();
  await page.getByRole("tab", { name: "Non-planar" }).click();
  await page.waitForTimeout(1200);
  await page.keyboard.press("Escape");
  await page.waitForTimeout(400);
  await openPalette(page);
  await page.locator(".ui-command-palette [cmdk-input]").fill("clear");
  await page.getByRole("option", { name: /Clear plate/i }).click();
  await page.waitForTimeout(1200);
  await page.keyboard.press("Control+K");
  await page.waitForTimeout(500);
  await page.locator(".ui-command-palette [cmdk-input]").fill("device");
  await page.waitForTimeout(400);
  await page.keyboard.press("Enter");
  await page.waitForTimeout(1200);
  await page.keyboard.press("Control+K");
  await page.waitForTimeout(500);
  await page.locator(".ui-command-palette [cmdk-input]").fill("home");
  await page.keyboard.press("Enter");
  await page.waitForTimeout(1500);
  await page.goto(`${base}/?mock=workspace-prepare&pane=prepare`, { waitUntil: "networkidle" });
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
await captureNonPlanar(page, "non-planar-panel-andromeda");
await captureToastUndo(page, "toast-undo-clear-plate");
await recordWalkthrough(browser);
await browser.close();
console.log(`Stage 3 review assets in ${outDir}`);