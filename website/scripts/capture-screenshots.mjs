import { chromium } from "playwright";
import { mkdir } from "node:fs/promises";
import { join } from "node:path";
import { coverageFromPng } from "./canvas-coverage.mjs";

const outDir =
  process.env.SCREENSHOT_OUT ?? join(process.cwd(), "docs/screenshots-redesign");
const baseUrl = process.env.SITE_URL ?? "http://127.0.0.1:4321";
const MIN_CANVAS_COVERAGE = Number(process.env.MIN_CANVAS_COVERAGE ?? "0.25");

const viewports = [
  { tag: "desktop", width: 1440, height: 900 },
  { tag: "mobile", width: 390, height: 844 },
];

const sections = [
  { id: "hero", selector: ".hero", fullPage: false, wait: 5000, settle: ".gcode-viewer--hero canvas" },
  { id: "coming-next", selector: "#coming-next", fullPage: false, wait: 4000, settle: "#coming-next canvas" },
  { id: "demos", selector: "#demos", fullPage: false, wait: 4500, settle: ".printer-row--rich" },
  {
    id: "toolpath",
    selector: "#toolpath",
    fullPage: false,
    wait: 3500,
    settle: ".demo-panel__viewer canvas",
    canvas: ".hero-print canvas",
    canvasAlt: ".demo-panel__viewer canvas",
  },
  { id: "printers", selector: "#printers", fullPage: false, wait: 2000, settle: ".printer-row--rich.is-active" },
  { id: "cli", selector: "#cli", fullPage: false, wait: 3500, settle: ".term-out" },
  { id: "downloads", selector: "#downloads", fullPage: false, wait: 600 },
  { id: "footer", selector: ".site-footer", fullPage: false, wait: 300 },
];

async function canvasInkCoverage(page, selector) {
  const loc = page.locator(selector).first();
  if ((await loc.count()) === 0) return { coverage: 0, litPixels: 0 };
  const box = await loc.boundingBox();
  if (!box) return { coverage: 0, litPixels: 0 };
  const buf = await page.screenshot({ clip: box, type: "png", animations: "disabled" });
  return coverageFromPng(buf);
}

await mkdir(outDir, { recursive: true });

const browser = await chromium.launch();
const regressions = [];

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
  await page.waitForSelector(".gcode-viewer--hero canvas", { timeout: 35000 });

  await page.waitForTimeout(3500);
  const heroCov = await canvasInkCoverage(page, ".gcode-viewer--hero canvas");
  console.log(`[${vp.tag}] hero canvas coverage=${heroCov.coverage?.toFixed(3)} lit=${heroCov.litPixels}`);
  if ((heroCov.coverage ?? 0) < MIN_CANVAS_COVERAGE) {
    regressions.push(`${vp.tag} hero coverage ${heroCov.coverage} < ${MIN_CANVAS_COVERAGE}`);
  }

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
    if (section.settle) {
      await page.waitForSelector(section.settle, { timeout: 15000 }).catch(() => {});
    }
    if (section.id === "toolpath") {
      await page
        .waitForFunction(
          () => document.querySelector("#toolpath .demo-panel__note")?.textContent?.includes("CuraEngine"),
          { timeout: 45000 },
        )
        .catch(() => {});
      await page.waitForTimeout(2500);
    } else {
      await page.waitForTimeout(section.wait);
    }

    if (section.id === "toolpath") {
      const toolCov = await canvasInkCoverage(page, ".demo-panel__viewer canvas");
      console.log(`[${vp.tag}] toolpath canvas coverage=${toolCov.coverage?.toFixed(3)} lit=${toolCov.litPixels}`);
      if ((toolCov.coverage ?? 0) < MIN_CANVAS_COVERAGE) {
        regressions.push(`${vp.tag} toolpath coverage ${toolCov.coverage} < ${MIN_CANVAS_COVERAGE}`);
      }
    }

    const path = join(outDir, `${section.id}-${vp.tag}.png`);
    try {
      await el.screenshot({ path, animations: "disabled", timeout: 45_000 });
    } catch {
      const box = await el.boundingBox();
      if (box) {
        await page.screenshot({ path, clip: box, animations: "disabled" });
      }
    }
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

if (regressions.length) {
  console.error("Visual regression failures:");
  for (const r of regressions) console.error(" -", r);
  process.exit(1);
}

console.log(`Screenshots in ${outDir}`);