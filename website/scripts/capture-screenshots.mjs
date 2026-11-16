import { chromium } from "playwright";
import { mkdir } from "node:fs/promises";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const outDir = "/opt/cursor/artifacts";
const baseUrl = process.env.SITE_URL ?? "http://127.0.0.1:4321";

const viewports = [
  { name: "site-1440x900", width: 1440, height: 900 },
  { name: "site-390x844", width: 390, height: 844 },
];

await mkdir(outDir, { recursive: true });

const browser = await chromium.launch();
for (const vp of viewports) {
  const page = await browser.newPage({
    viewport: { width: vp.width, height: vp.height },
  });
  await page.goto(baseUrl, { waitUntil: "networkidle" });
  await page.waitForTimeout(1200);
  await page.screenshot({
    path: join(outDir, `${vp.name}.png`),
    fullPage: true,
  });
  await page.close();
}
await browser.close();

console.log(`Wrote screenshots to ${outDir}`);