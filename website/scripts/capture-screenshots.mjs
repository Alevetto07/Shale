import { chromium } from "playwright";
import { mkdir } from "node:fs/promises";
import { join } from "node:path";

const outDir = "/opt/cursor/artifacts";
const baseUrl = process.env.SITE_URL ?? "http://127.0.0.1:4321";

const jobs = [
  {
    name: "site-1440x900",
    width: 1440,
    height: 900,
    fullPage: true,
    heroOnly: false,
  },
  {
    name: "site-1440x900-hero",
    width: 1440,
    height: 900,
    fullPage: false,
    heroOnly: true,
  },
  {
    name: "site-390x844",
    width: 390,
    height: 844,
    fullPage: true,
    heroOnly: false,
  },
  {
    name: "site-390x844-hero",
    width: 390,
    height: 844,
    fullPage: false,
    heroOnly: true,
  },
];

await mkdir(outDir, { recursive: true });

const browser = await chromium.launch();
for (const job of jobs) {
  const page = await browser.newPage({
    viewport: { width: job.width, height: job.height },
  });
  await page.goto(baseUrl, { waitUntil: "networkidle" });
  await page.waitForTimeout(job.heroOnly ? 900 : 600);

  const path = join(outDir, `${job.name}.png`);
  if (job.heroOnly) {
    const hero = page.locator("section.hero");
    await hero.screenshot({ path });
  } else {
    await page.screenshot({ path, fullPage: job.fullPage });
  }
  await page.close();
}
await browser.close();

console.log(`Wrote ${jobs.length} screenshots to ${outDir}`);