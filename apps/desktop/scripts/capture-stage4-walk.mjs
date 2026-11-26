import { chromium } from "playwright";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { mkdirSync } from "node:fs";

const base = process.env.SHALE_CAPTURE_URL ?? "http://127.0.0.1:1420";
const here = path.dirname(fileURLToPath(import.meta.url));
const outDir = path.resolve(here, "../review-screens");
mkdirSync(outDir, { recursive: true });

const browser = await chromium.launch();

const still = await browser.newPage({ viewport: { width: 1440, height: 900 } });
async function goto(pathAndQuery) {
  await still.goto(`${base}${pathAndQuery}`, { waitUntil: "networkidle", timeout: 60_000 });
  await still.waitForTimeout(600);
}
for (const [query, name] of [
  ["", "andromeda"],
  ["&uiStyle=saturn", "saturn"],
  ["&uiStyle=nebula", "nebula"],
  ["&uiStyle=lunar", "lunar"],
]) {
  await goto(`/?mock=workspace-prepare&pane=prepare&models=1${query}`);
  await still.getByRole("button", { name: "Settings", exact: true }).click();
  await still.waitForTimeout(350);
  await still.screenshot({ path: `${outDir}/stage4-settings-${name}.png` });
  await still.keyboard.press("Escape");
}
await goto("/?mock=device-lan&pane=device");
await still.screenshot({ path: `${outDir}/stage4-device-andromeda.png` });
await goto("/?mock=workspace-prepare&pane=prepare&models=1");
await still.getByRole("banner").getByRole("button", { name: "Collapse sidebar" }).click();
await still.waitForTimeout(700);
await still.screenshot({ path: `${outDir}/stage4-sidebar-collapsed-andromeda.png` });
await still.close();

const context = await browser.newContext({
  viewport: { width: 1440, height: 900 },
  recordVideo: { dir: outDir, size: { width: 1440, height: 900 } },
});
const page = await context.newPage();
const t0 = Date.now();
const mark = (label) => console.log(`${((Date.now() - t0) / 1000).toFixed(1)}s ${label}`);

await page.goto(`${base}/?mock=workspace-preview&pane=prepare&models=1`, {
  waitUntil: "networkidle",
  timeout: 60_000,
});
await page.waitForTimeout(1100);
mark("ready");

await page.getByRole("banner").getByRole("button", { name: "Collapse sidebar" }).click();
await page.waitForTimeout(1100);
mark("collapsed");
await page.getByRole("banner").getByRole("button", { name: "Expand sidebar" }).click();
await page.waitForTimeout(1100);
mark("expanded");

await page.getByRole("tab", { name: "Preview" }).click();
await page.waitForTimeout(600);
mark("preview");

const slider = page.getByRole("slider").first();
const box = await slider.boundingBox();
if (box) {
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width / 2 + 80, box.y + box.height / 2, { steps: 8 });
  await page.mouse.up();
}
await page.waitForTimeout(400);
mark("slider");
await page.getByRole("tab", { name: "Device" }).click();
await page.waitForTimeout(650);
mark("device");
await page.getByRole("tab", { name: "Prepare" }).click();
await page.waitForTimeout(450);
mark("prepare");

await page.keyboard.press("Control+K");
await page.waitForSelector(".ui-command-palette");
await page.waitForTimeout(500);
mark("palette");
await page.locator(".ui-command-palette [cmdk-input]").fill("clear plate");
await page.waitForTimeout(250);
await page.getByRole("option", { name: /Clear plate/i }).click();
await page.waitForTimeout(700);
mark("toast");
await page.getByRole("button", { name: "Undo" }).click();
await page.waitForTimeout(700);
mark("undo");

for (const theme of ["Saturn", "Nebula", "Lunar", "Andromeda"]) {
  await page.keyboard.press("Control+K");
  await page.waitForSelector(".ui-command-palette");
  await page.locator(".ui-command-palette [cmdk-input]").fill("theme");
  await page.getByRole("option", { name: /Toggle theme/i }).click();
  await page.waitForTimeout(1400);
  mark(theme);
}

await page.waitForTimeout(4200);
mark("end");
const video = page.video();
await context.close();
if (video) await video.saveAs(`${outDir}/ui-rework-stage4-walkthrough.webm`);
await browser.close();
console.log("wrote walkthrough");
