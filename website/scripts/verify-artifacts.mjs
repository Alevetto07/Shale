import { chromium } from "playwright";
import { mkdir } from "node:fs/promises";
import { join } from "node:path";
import { coverageFromPng } from "./canvas-coverage.mjs";

const out = "/opt/cursor/artifacts";
const url = process.env.SITE_URL ?? "http://127.0.0.1:4321";

await mkdir(out, { recursive: true });

async function waitPreview(page, tag) {
  await page.waitForSelector("#app-demo", { timeout: 45000 });
  await page.evaluate(() => {
    document.querySelectorAll("[data-reveal]").forEach((el) => el.classList.add("is-visible"));
  });
  await page.locator("#app-demo").scrollIntoViewIfNeeded();
  await page.waitForSelector("#app-demo .gcode-viewer canvas", { timeout: 60000 });
  await page.waitForTimeout(tag === "mobile" ? 4000 : 3000);
}

const browser = await chromium.launch();

for (const { tag, width, height } of [
  { tag: "desktop", width: 1440, height: 900 },
  { tag: "mobile", width: 390, height: 844 },
]) {
  const page = await browser.newPage({ viewport: { width, height } });
  await page.goto(url, { waitUntil: "networkidle" });
  await waitPreview(page, tag);
  await page.locator(".hero-narrow").screenshot({ path: join(out, `hero-${tag}.png`) });
  await page.locator("#app-demo").screenshot({ path: join(out, `app-demo-${tag}.png`) });
  const appBox = await page.locator("#app-demo .gcode-viewer canvas").boundingBox();
  if (appBox) {
    const c = coverageFromPng(await page.screenshot({ clip: appBox, type: "png" }));
    console.log(`[${tag}] app-preview coverage=${c.coverage.toFixed(3)}`);
  }
  await page.locator(".section-tight").nth(0).scrollIntoViewIfNeeded();
  await page.waitForSelector("#activity-title", { timeout: 45000 });
  await page.waitForTimeout(1000);
  await page.locator("#activity").screenshot({ path: join(out, `timeline-${tag}.png`) });
  await page.locator("#demos [role='tab']", { hasText: "Workbench" }).click();
  await page.locator("#toolpath").scrollIntoViewIfNeeded({ block: "center" });
  await page.waitForTimeout(tag === "mobile" ? 7000 : 4500);
  await page.locator("#toolpath").screenshot({ path: join(out, `toolpath-${tag}.png`) });
  const toolBox = await page.locator("#toolpath .demo-panel__viewer canvas").boundingBox();
  if (toolBox) {
    const c = coverageFromPng(await page.screenshot({ clip: toolBox, type: "png" }));
    console.log(`[${tag}] toolpath coverage=${c.coverage.toFixed(3)}`);
  }
  await page.close();
}

await browser.close();
console.log(`Artifacts written under ${out}`);