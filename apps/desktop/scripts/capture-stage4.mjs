import { chromium } from "playwright";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { mkdirSync } from "node:fs";

const base = process.env.SHALE_CAPTURE_URL ?? "http://127.0.0.1:1420";
const here = path.dirname(fileURLToPath(import.meta.url));
const outDir = path.resolve(here, "../review-screens");
mkdirSync(outDir, { recursive: true });

const themes = [
  ["", "andromeda"],
  ["&uiStyle=saturn", "saturn"],
  ["&uiStyle=nebula", "nebula"],
  ["&uiStyle=lunar", "lunar"],
];

async function shot(page, file) {
  await page.screenshot({ path: `${outDir}/${file}.png` });
}

async function goto(page, pathAndQuery) {
  await page.goto(`${base}${pathAndQuery}`, { waitUntil: "networkidle", timeout: 60_000 });
  await page.waitForTimeout(700);
}

const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });

for (const [query, name] of themes) {
  await goto(page, `/?mock=home&pane=home&plateStill=1${query}`);
  await shot(page, `stage4-home-${name}`);
  await goto(page, `/?mock=workspace-prepare&pane=prepare&models=1${query}`);
  await shot(page, `stage4-prepare-${name}`);
}

await goto(page, "/?mock=workspace-preview&pane=preview");
await shot(page, "stage4-preview-andromeda");
await page.getByRole("button", { name: "Collapse sidebar" }).first().click();
await page.waitForTimeout(600);
await shot(page, "stage4-sidebar-collapsed-andromeda");
await page.getByRole("button", { name: "Expand sidebar" }).first().click();
await page.waitForTimeout(500);

await goto(page, "/?mock=workspace-prepare&pane=prepare&models=1");
await page.getByRole("button", { name: "Slice settings" }).click({ timeout: 8000 });
await page.waitForTimeout(400);
await page.getByRole("tab", { name: "Non-planar" }).click();
await page.waitForTimeout(300);
await shot(page, "stage4-slice-andromeda");
await page.keyboard.press("Escape");

await page.getByRole("tab", { name: "Home" }).click();
await page.waitForTimeout(400);
await page.getByRole("button", { name: "Settings" }).first().click();
await page.waitForTimeout(400);
await shot(page, "stage4-settings-andromeda");

await goto(page, "/?mock=workspace-prepare&pane=prepare&models=1&uiStyle=lunar");
await page.keyboard.press("Control+K");
await page.waitForSelector(".ui-command-palette");
await shot(page, "stage4-palette-lunar");
await page.keyboard.press("Escape");

await goto(page, "/?mock=workspace-prepare&pane=prepare&models=1");
await page.keyboard.press("Control+K");
await page.locator(".ui-command-palette [cmdk-input]").fill("clear plate");
await page.getByRole("option", { name: /Clear plate/i }).click();
await page.waitForTimeout(400);
await shot(page, "stage4-toast-andromeda");

await browser.close();
console.log(outDir);
