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
await page.goto(url, { waitUntil: "networkidle" });
await page.evaluate(() => {
  document.querySelectorAll("[data-reveal], .slogan-display").forEach((el) => {
    el.classList.add("is-visible");
  });
});

await page.waitForSelector(".gcode-viewer--hero canvas", { timeout: 35000 });
await page.waitForTimeout(2000);

for (const i of [1, 2, 3]) {
  await page.locator(".field-picker-btn").nth(i).click();
  await page.waitForTimeout(700);
}

await page.mouse.move(640, 280);
await page.mouse.move(820, 360, { steps: 16 });
await page.waitForTimeout(600);

await page.locator("#demos").scrollIntoViewIfNeeded();
await page.waitForTimeout(2500);

await page.getByRole("button", { name: /Play layers/i }).click();
await page.waitForTimeout(2800);

await page.getByLabel("Travel").check();
await page.waitForTimeout(600);
await page.getByLabel("Travel").uncheck();
await page.waitForTimeout(500);

await page.locator("#toolpath input[type='range']").first().fill("95");
await page.waitForTimeout(800);

await page.getByPlaceholder("Search").fill("p1");
await page.getByRole("option", { name: /P1S/i }).click();
await page.waitForTimeout(900);

await page.locator("#slice-settings input[type='range']").nth(1).fill("32");
await page.waitForTimeout(600);

await page.locator("#cli").scrollIntoViewIfNeeded();
await page.waitForSelector(".term-out", { timeout: 15000 });
await page.waitForTimeout(2200);

await page.locator("#mcp").scrollIntoViewIfNeeded();
await page.waitForTimeout(2200);

await page.locator("#coming-next").scrollIntoViewIfNeeded();
await page.waitForTimeout(2000);
await page.locator(".compare-slider input").first().fill("70");
await page.waitForTimeout(1200);

await page.getByRole("button", { name: /Run comparison/i }).click();
await page.waitForTimeout(2200);

await page.evaluate(() => window.scrollTo({ top: 0, behavior: "smooth" }));
await page.waitForTimeout(1800);

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