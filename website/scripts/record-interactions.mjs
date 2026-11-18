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
await page.waitForTimeout(600);

await page.getByRole("button", { name: /Preview layers/i }).click();
await page.waitForTimeout(2500);

await page.getByRole("button", { name: /Slice/i }).click();
await page.waitForTimeout(800);
await page.getByRole("button", { name: /Settings/i }).click();
await page.waitForTimeout(800);

await page.locator(".workflow-pause").click();
await page.waitForTimeout(400);

for (const i of [0, 1, 2]) {
  await page.locator(".field-picker-btn").nth(i).click();
  await page.waitForTimeout(450);
}

await page.locator(".workflow-pause").click();
await page.waitForTimeout(3200);

await page.locator("#activity").scrollIntoViewIfNeeded();
await page.getByRole("button", { name: /^Play$/i }).click();
await page.waitForTimeout(1400);

await page.locator("#demos").scrollIntoViewIfNeeded();
await page.waitForTimeout(600);
await page.getByRole("tab", { name: /CLI/i }).click();
await page.waitForTimeout(700);

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