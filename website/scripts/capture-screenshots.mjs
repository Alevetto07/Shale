import { chromium } from "playwright";
import { mkdir } from "node:fs/promises";
import { join } from "node:path";

const outDir =
  process.env.SCREENSHOT_OUT ?? join(process.cwd(), "docs/screenshots-redesign");
const baseUrl = process.env.SITE_URL ?? "http://127.0.0.1:4321";

const viewports = [
  { tag: "desktop", width: 1440, height: 900 },
  { tag: "mobile", width: 390, height: 844 },
];

const sections = [
  { id: "hero", selector: ".hero", fullPage: false, wait: 1600 },
  { id: "features", selector: "#features", fullPage: false, wait: 400 },
  { id: "media", selector: "#media", fullPage: false, wait: 400 },
  { id: "downloads", selector: "#downloads", fullPage: false, wait: 400 },
  { id: "coming", selector: "#coming", fullPage: false, wait: 400 },
  { id: "footer", selector: ".site-footer", fullPage: false, wait: 300 },
];

await mkdir(outDir, { recursive: true });

const browser = await chromium.launch();
for (const vp of viewports) {
  const page = await browser.newPage({
    viewport: { width: vp.width, height: vp.height },
  });
  await page.goto(baseUrl, { waitUntil: "networkidle" });
  await page.evaluate(() => {
    document.querySelectorAll("[data-reveal], .slogan-display").forEach((el) => {
      el.classList.add("is-visible");
    });
  });

  for (const section of sections) {
    const el = await page.$(section.selector);
    if (!el) {
      console.warn(`Missing ${section.selector}`);
      continue;
    }
    const headerPad = 88;
    await page.evaluate(
      ({ sel, pad }) => {
        const node = document.querySelector(sel);
        if (!node) return;
        const y = node.getBoundingClientRect().top + window.scrollY - pad;
        window.scrollTo({ top: Math.max(0, y), behavior: "instant" });
      },
      { sel: section.selector, pad: headerPad },
    );
    await page.waitForTimeout(section.wait);
    const path = join(outDir, `${section.id}-${vp.tag}.png`);
    await el.screenshot({ path });
    console.log(`Wrote ${path}`);
  }

  const fullPath = join(outDir, `full-page-${vp.tag}.png`);
  await page.evaluate(() => window.scrollTo(0, 0));
  await page.waitForTimeout(500);
  await page.screenshot({ path: fullPath, fullPage: true });
  console.log(`Wrote ${fullPath}`);

  await page.close();
}
await browser.close();

console.log(`Screenshots in ${outDir}`);