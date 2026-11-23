import { chromium } from "playwright";
import { mkdir } from "node:fs/promises";

const base = process.env.SHALE_CAPTURE_URL ?? "http://127.0.0.1:5173";
const outDir = new URL("../review-screens/", import.meta.url).pathname;

const shots = [
  { path: "/?mock=workspace-prepare&pane=prepare", name: "workspace-andromeda-prepare" },
  { path: "/?mock=workspace-prepare&pane=prepare&uiStyle=deep-field", name: "start-andromeda" },
  { path: "/?mock=workspace-prepare&pane=home", name: "home-andromeda-sidebar" },
  { path: "/?mock=workspace-prepare&pane=home&uiStyle=lunar", name: "home-lunar-sidebar" },
  { path: "/?mock=workspace-preview&pane=preview", name: "workspace-andromeda-preview" },
];

await mkdir(outDir, { recursive: true });
const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });

for (const shot of shots) {
  await page.goto(`${base}${shot.path}`, { waitUntil: "networkidle", timeout: 60_000 });
  await page.waitForTimeout(1200);
  await page.screenshot({ path: `${outDir}${shot.name}.png`, fullPage: false });
}

const videoCtx = await browser.newContext({
  viewport: { width: 1440, height: 900 },
  recordVideo: { dir: outDir, size: { width: 1440, height: 900 } },
});
const videoPage = await videoCtx.newPage();
await videoPage.goto(`${base}/?mock=workspace-prepare&pane=home`, { waitUntil: "networkidle", timeout: 60_000 });
await videoPage.waitForTimeout(800);
await videoPage.locator(".ui-sidebar__row").first().hover().catch(() => undefined);
await videoPage.waitForTimeout(400);
await videoPage.getByRole("button", { name: /New project/i }).click({ timeout: 5000 }).catch(() => undefined);
await videoPage.waitForTimeout(600);
await videoPage.getByRole("button", { name: /Collapse sidebar/i }).click({ timeout: 5000 }).catch(() => undefined);
await videoPage.waitForTimeout(800);
await videoCtx.close();

await browser.close();
console.log(`Wrote ${shots.length} screenshots and a sidebar motion clip to ${outDir}`);