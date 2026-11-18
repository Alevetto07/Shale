import { chromium } from "playwright";
import { mkdir } from "node:fs/promises";
import { join } from "node:path";

const out = "/opt/cursor/artifacts";
const url = process.env.SITE_URL ?? "http://127.0.0.1:4321";

await mkdir(out, { recursive: true });

const browser = await chromium.launch();
const context = await browser.newContext({
  recordVideo: { dir: out, size: { width: 1280, height: 720 } },
  viewport: { width: 1280, height: 720 },
});
const page = await context.newPage();
await page.goto(url, { waitUntil: "domcontentloaded" });
await page.evaluate(() => {
  document.querySelectorAll("[data-reveal], .slogan-display").forEach((el) => {
    el.classList.add("is-visible");
  });
});

await page.waitForSelector("#app-demo", { timeout: 45000 });
await page.locator(".workflow-timeline button").nth(5).click();
await page.locator(".workflow-pause").click();
await page.waitForSelector("#app-demo .gcode-viewer canvas", { timeout: 45000 });
await page.waitForTimeout(1200);

await page.locator(".workflow-pause").click();
await page.waitForTimeout(300);
await page.locator(".field-picker-btn").nth(1).click();
await page.waitForTimeout(400);
await page.locator(".workflow-pause").click();
await page.waitForTimeout(2800);

await page.locator("#activity").scrollIntoViewIfNeeded();
await page.getByRole("button", { name: /^Play$/i }).click();
await page.waitForTimeout(1500);

await page.locator("#demos").scrollIntoViewIfNeeded();
await page.locator("#demos [role='tab']", { hasText: "Workbench" }).click();
await page.waitForSelector("#toolpath .demo-panel__viewer canvas", { timeout: 45000 });
await page.locator("#toolpath").scrollIntoViewIfNeeded();
await page.waitForTimeout(2000);
await page.getByLabel("Travel").check();
await page.waitForTimeout(400);
await page.getByLabel("Travel").uncheck();
await page.waitForTimeout(300);

const video = page.video();
await page.close();
await context.close();
await browser.close();

if (video) {
  const vp = await video.path();
  const target = join(out, "shale-demos-interaction.webm");
  const { rename } = await import("node:fs/promises");
  await rename(vp, target);
  console.log(`Wrote ${target}`);
}